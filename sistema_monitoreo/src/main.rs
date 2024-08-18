use std::{io::BufRead, thread};

use sistema_monitoreo::errores::SistemaMonitoreoError;

use crate::sistema_monitoreo::sistema_monitoreo::SistemaDeMonitoreo;
use std::time::Duration;

pub mod sistema_monitoreo;

fn main() {
    if let Err(e) = sistema_monitoreo_run(&mut std::io::stdin()) {
        println!("Error: {:?}", e);
    }
}

/// Esta funcion solo se agrega a modo de prueba para poder probar el sistema de monitoreo por separado
/// Si realmente se quiere usar se importa como libreria en la ui y se usa desde ahi
fn sistema_monitoreo_run(stream: &mut dyn std::io::Read) -> Result<(), SistemaMonitoreoError> {
    let mut sist = SistemaDeMonitoreo::new("sismonitoreo".to_string(), "prueba123".to_string())?;

    let id = sist.nuevo_incidente(1.0, 1.0, 10)?;
    let _ = sist.nuevo_incidente(1.0, 1.0, 10);
    let _ = sist.nuevo_incidente(3.0, 2.0, 12);

    println!(
        "Cantidad Incidentes: {}",
        sist.obtener_cant_de_incidentes()?
    );
    println!("Incidentes: {:?}", sist.obtener_info_incidentes());

    sist.eliminar_incidente(&id)?;
    println!(
        "Cantidad Incidentes despues de eliminar el de id {}: {}",
        id,
        sist.obtener_cant_de_incidentes()?
    );
    println!(
        "Incidentes despues de eliminar: {:?}",
        sist.obtener_info_incidentes()
    );

    println!(
        "Listado camaras antes de agregar: {:?}",
        sist.obtener_info_camaras()
    );
    thread::sleep(Duration::from_secs(10));
    let _ = sist.nuevo_incidente(19.0, 19.0, 12);

    println!(
        "Listado camaras despues de agregar: {:?}",
        sist.obtener_info_camaras()
    );
    thread::sleep(Duration::from_secs(10));
    println!(
        "Listado camaras despues de modificar estado: {:?}",
        sist.obtener_info_camaras()
    );

    let _reader = std::io::BufReader::new(stream);

    for line in _reader.lines() {
        if line.is_ok() {
            println!("Sistema de Monitoreo: {:?}", sist.obtener_info_camaras());
        }
    }
    Ok(())
}
