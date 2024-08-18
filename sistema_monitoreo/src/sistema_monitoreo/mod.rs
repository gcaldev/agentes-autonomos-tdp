pub mod camara;
pub mod dron;
pub mod errores;
pub mod estado;
pub mod incidente;
pub mod posicion;
#[allow(clippy::module_inception)]
pub mod sistema_monitoreo;
pub use sistema_monitoreo::SistemaDeMonitoreo;
