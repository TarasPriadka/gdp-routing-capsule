use std::net::Ipv4Addr;

/*
* Copyright 2019 Comcast Cable Communications Management, LLC
*
* Licensed under the Apache License, Version 2.0 (the "License");
* you may not use this file except in compliance with the License.
* You may obtain a copy of the License at
*
* http://www.apache.org/licenses/LICENSE-2.0
*
* Unless required by applicable law or agreed to in writing, software
* distributed under the License is distributed on an "AS IS" BASIS,
* WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
* See the License for the specific language governing permissions and
* limitations under the License.
*
* SPDX-License-Identifier: Apache-2.0
*/
use crate::dtls::{decrypt_gdp, encrypt_gdp};
use crate::gdp::Gdp;
use crate::gdp::GdpAction;
use crate::kvs::Store;
use crate::pipeline::GdpPipeline;

use crate::rib::handle_rib_query;
use crate::rib::handle_rib_reply;
use anyhow::anyhow;
use anyhow::Result;


use capsule::batch::{Batch, Pipeline, Poll};

use capsule::config::load_config;
use capsule::packets::ip::v4::Ipv4;
use capsule::packets::ip::IpPacket;
use capsule::packets::Udp;
use capsule::packets::{Ethernet, Packet};
use capsule::{PortQueue, Runtime};


use tracing::Level;
use tracing_subscriber::fmt;

mod dtls;
mod gdp;
mod kvs;
mod pipeline;
mod rib;

fn find_destination(gdp: &Gdp<Ipv4>, store: Store) -> Option<Ipv4Addr> {
    store.with_contents(|store| store.forwarding_table.get(&gdp.dst()).cloned())
}

fn bounce_udp(udp: &mut Udp<Ipv4>) -> &mut Udp<Ipv4> {
    let udp_src_port = udp.dst_port();
    let udp_dst_port = udp.src_port();
    udp.set_src_port(udp_src_port);
    udp.set_dst_port(udp_dst_port);

    let ethernet = udp.envelope_mut();
    let eth_src = ethernet.dst();
    let eth_dst = ethernet.src();
    ethernet.set_src(eth_src);
    ethernet.set_dst(eth_dst);

    udp
}

fn forward_gdp(mut gdp: Gdp<Ipv4>, dst: Ipv4Addr) -> Result<Gdp<Ipv4>> {
    let udp = gdp.envelope_mut();
    let ipv4 = udp.envelope_mut();

    ipv4.set_src(ipv4.dst());
    ipv4.set_dst(dst);

    Ok(gdp)
}

fn bounce_gdp(mut gdp: Gdp<Ipv4>) -> Result<Gdp<Ipv4>> {
    gdp.remove_payload()?;
    gdp.set_action(GdpAction::Nack);
    bounce_udp(gdp.envelope_mut());
    gdp.reconcile_all();
    Ok(gdp)
}

fn switch_pipeline(store: Store) -> impl GdpPipeline {
    return pipeline! {
        GdpAction::Forward => |group| {
            group.group_by(
                move |packet| find_destination(packet, store).is_some(),
                pipeline! {
                    true => |group| {group.map(move |packet| {
                        let dst = find_destination(&packet, store).ok_or(anyhow!("can't find the destination"))?;
                        forward_gdp(packet, dst)
                    })}
                    false => |group| {group.map(bounce_gdp)}//.emit(create_rib_request(Mbuf::new(), pack))}
                })
        }
        GdpAction::RibReply => |group| {
            group.for_each(move |packet| handle_rib_reply(packet, store))
                .filter(|_| false)
        }
        _ => |group| {group.filter(|_| false)}
    };
}

fn rib_pipeline(store: Store) -> impl GdpPipeline {
    return pipeline! {
        GdpAction::RibGet => |group| {
            group.replace(move |packet| handle_rib_query(packet, store))
        }
        _ => |group| {group.filter(|_| false)}
    };
}

fn install_gdp_pipeline<T: GdpPipeline>(q: PortQueue, gdp_pipeline: T) -> impl Pipeline {
    Poll::new(q.clone())
        .map(|packet| {
            Ok(packet
                .parse::<Ethernet>()?
                .parse::<Ipv4>()?
                .parse::<Udp<Ipv4>>()?)
        })
        .map(|packet| decrypt_gdp(packet))
        .map(|packet| Ok(packet.parse::<Gdp<Ipv4>>()?))
        .group_by(
            |packet| packet.action().unwrap_or(GdpAction::Noop),
            gdp_pipeline,
        )
        .map(|packet| {
            encrypt_gdp(packet.deparse()) // obviously this doesn't work
        })
        .send(q)
}

fn main() -> Result<()> {
    let subscriber = fmt::Subscriber::builder()
        .with_max_level(Level::DEBUG)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let config = load_config()?;

    let store1 = Store::new();
    let store2 = Store::new();

    Runtime::build(config)?
        .add_pipeline_to_port("eth1", move |q| {
            install_gdp_pipeline(q, switch_pipeline(store1))
        })?
        .add_pipeline_to_port("eth2", move |q| {
            install_gdp_pipeline(q, rib_pipeline(store2))
        })?
        .execute()
}
