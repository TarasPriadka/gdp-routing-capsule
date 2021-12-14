use std::time::Duration;

use anyhow::Result;
use capsule::config::RuntimeConfig;
use capsule::Runtime;

use crate::gdp_pipeline::install_gdp_pipeline;
use crate::hardcoded_routes::{load_routes, startup_route_lookup};
use crate::kvs::Store;
use crate::pipeline::GdpPipeline;
use crate::rib::{rib_pipeline, Routes};
use crate::statistics::{dump_history, make_print_stats};
use crate::switch::switch_pipeline;

pub enum ProdMode {
    Router,
    Switch,
}

pub fn start_prod_server(
    config: RuntimeConfig,
    mode: ProdMode,
    gdp_index: Option<u8>,
    use_default: bool,
) -> Result<()> {
    fn create_rib(_store: Store, routes: &'static Routes, use_default: bool) -> impl GdpPipeline {
        rib_pipeline("rib", routes, use_default, false)
    }

    fn create_switch(store: Store, routes: &'static Routes, _: bool) -> impl GdpPipeline {
        switch_pipeline(store, "switch", routes, routes.rib, false)
    }

    fn start<T: GdpPipeline + 'static>(
        config: RuntimeConfig,
        gdp_index: Option<u8>,
        use_default: bool,
        pipeline: fn(Store, &'static Routes, bool) -> T,
    ) -> Result<()> {
        let node_addr = gdp_index.and_then(startup_route_lookup);

        let store = Store::new_shared();
        let (print_stats, history_map) = make_print_stats();
        let routes: &'static Routes = Box::leak(Box::new(load_routes()?));

        Runtime::build(config)?
            .add_pipeline_to_port("eth1", move |q| {
                let store = store.sync();
                install_gdp_pipeline(
                    q,
                    pipeline(store, routes, use_default),
                    store,
                    "prod",
                    node_addr,
                    false,
                )
            })?
            .add_periodic_task_to_core(0, print_stats, Duration::from_secs(1))?
            .add_periodic_task_to_core(
                0,
                move || store.run_active_expire(),
                Duration::from_secs(1),
            )?
            .execute()?;
        dump_history(&(*history_map.lock().unwrap()))?;
        Ok(())
    }

    match mode {
        ProdMode::Router => start(config, gdp_index, use_default, create_rib),
        ProdMode::Switch => start(config, gdp_index, use_default, create_switch),
    }
}