use crate::camara::central_de_camaras::CentralDeCamaras;
use std::io::BufRead;
use std::io::{stdin, BufReader, Read};
mod camara;

/// Este main solo se agrega a modo de prueba para poder probar la central de camaras por separado
/// Si realmente se quiere usar se importa como libreria en la ui y se usa desde ahi
fn main() {
    if let Ok(mut central) = CentralDeCamaras::new() {
        let _ = central.nueva_camara(5.0, 5.0);
        let _ = central.nueva_camara(20.0, 20.0);
        central_run(&mut stdin(), central);
    }
}

pub fn central_run(stream: &mut dyn Read, central_llego: CentralDeCamaras) {
    let _reader = BufReader::new(stream);

    for line in _reader.lines() {
        if line.is_ok() {
            println!("Central: {:?}", central_llego.lista_de_camaras);
        }
    }
}
