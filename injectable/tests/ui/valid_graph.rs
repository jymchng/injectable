//! Compile-pass test: a valid dependency graph.
//!
//! This graph has no cycles, no scope mismatches, no missing
//! dependencies, and no duplicates. It should compile successfully.

use injectable::{container, Injectable, Inject};

#[derive(Injectable, Default, Clone)]
pub struct Database;

#[derive(Injectable, Default, Clone)]
pub struct Config;

#[derive(Injectable)]
pub struct UserService {
    db: Inject<Database>,
    config: Inject<Config>,
}

fn main() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let container = container! {
            Database,
            Config,
            UserService { deps: [Database, Config] },
        };
        let container = container.await.unwrap();

        let _service = container.resolve::<UserService>().await.unwrap();
    });
}
