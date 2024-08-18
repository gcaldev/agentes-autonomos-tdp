use detector_incidentes::es_incidente;
use std::fs::File;
use std::io::Read;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file_path = "robo2.jpg";
    let mut file = File::open(file_path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    match es_incidente(&buffer) {
        Ok(incidente) => {
            if incidente {
                println!("Incidente Detectado.");
            } else {
                println!("No se detecto un incidente.");
            }
        }
        Err(e) => println!("Error: {:?}", e),
    }

    Ok(())
}
