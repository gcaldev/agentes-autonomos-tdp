use std::env::args;

use structs::server::Server;
mod enums;
mod structs;

static SERVER_ARGS: usize = 2;

/// Se ejecuta al server al ejecutar este archivo y pasarle como argumento el numero para identificarlo.
fn main() -> Result<(), ()> {
    //1. Lectura de argumentos:
    let argv = args().collect::<Vec<String>>();
    if argv.len() < SERVER_ARGS {
        println!("Cantidad de argumentos inválido");
        let app_name: &String = &argv[0];
        println!("Usage:\n{:?} <puerto>", app_name);
        return Err(());
    }

    let address = "127.0.0.1:".to_owned() + &argv[1];
    let mut server = Server::new(&address);

    if let Err(e) = server.run() {
        println!("Error: {:?}", e);
    }
    Ok(())
}
