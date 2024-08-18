use std::{
    env::args,
    io::{stdin, BufRead, BufReader, Read, Write},
    net::TcpStream,
    sync::{Arc, Mutex},
};

use cliente::cliente::client::NatsClient;
use cliente::cliente::iclient::INatsClient;
use cliente::cliente::user::User;
static CLIENT_ARGS: usize = 3;

/// Este main solo se agrega a modo de prueba para poder probar el cliente de nats por separado
/// Si realmente se quiere usar se importa como libreria en otro proyecto y se usa desde ahi
fn main() -> std::io::Result<()> {
    let argv = args().collect::<Vec<String>>();
    if argv.len() != CLIENT_ARGS {
        println!("Cantidad de argumentos inválido");
        let app_name = &argv[0];
        println!("{:?} <host> <puerto>", app_name);
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Cantidad de argumentos inválida",
        ));
    }

    let address = argv[1].clone() + ":" + &argv[2];
    println!("Conectándome a {:?}", address);

    client_run(&address, &mut stdin())?;
    Ok(())
}

fn client_run(address: &str, stream: &mut dyn Read) -> std::io::Result<()> {
    let writer = TcpStream::connect(address)?;
    let reader = writer.try_clone()?;

    let nats = NatsClient::new(
        writer,
        reader,
        "logs.txt",
        Some(User::new("b".to_string(), "prueba123".to_string())),
    )?;
    let nats_ref = Arc::new(Mutex::new(nats));
    let _reader = BufReader::new(stream);

    let mut writer = TcpStream::connect(address)?;

    for line in _reader.lines().map_while(Result::ok) {
        let splitted_line: Vec<&str> = line.split(' ').collect();
        let mut nats = nats_ref.lock().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::Other, "Error al lockear el mutex")
        })?;

        match splitted_line.first() {
            Some(&"PUB") => {
                if let Some(subject) = splitted_line.get(1) {
                    let mut payload = splitted_line.get(2); // PUB time payload time.us

                    let hardcoded_payload = false;

                    let reply_to = splitted_line.get(3);

                    if hardcoded_payload {
                        payload = if *subject == "Posicion_Camaras" {
                            Some(&"0 12 21")
                        } else {
                            Some(&"0 Inactiva")
                        };
                    }

                    let _ = nats.publish(subject, payload.copied(), reply_to.copied());
                }
            }
            Some(&"SUB") => {
                if let Some(subject) = splitted_line.get(1) {
                    let nats_ref_clone = Arc::clone(&nats_ref);

                    let sub_id = nats.subscribe(
                        subject,
                        Box::new(move |payload| {
                            println!("Payload: {}", payload.0);
                            if let Ok(mut nats) = nats_ref_clone.lock() {
                                if let Some(reply_to) = &payload.1 {
                                    let _ = nats.publish(reply_to, Some("Respuesta"), None);
                                }
                            }
                        }),
                    )?;
                    println!("Sub_id: {}", sub_id);
                }
            }
            Some(&"UNSUB") => {
                if let Some(sub_id_str) = splitted_line.get(1) {
                    let sub_id_str = sub_id_str.trim();
                    if let Ok(sub_id) = sub_id_str.parse::<usize>() {
                        let max_msgs = splitted_line.get(2).and_then(|s| s.parse().ok());
                        nats.unsubscribe(sub_id, max_msgs)?;
                    }
                }
            }
            Some(&"HPUB") => {
                if let Some(subject) = splitted_line.get(1) {
                    if let Some(headers) = splitted_line.get(2) {
                        if let Some(payload) = splitted_line.get(3) {
                            println!("Headers: {}", headers);
                            println!("Payload: {}", payload);
                            println!("Subject: {}", subject);
                            let _ = nats.hpublish(subject, headers, payload, None);
                            drop(nats);
                        }
                    }
                }
            }
            Some(&"CREATE_STREAM") => {
                if let Some(stream_name) = splitted_line.get(1) {
                    if let Some(subject) = splitted_line.get(2) {
                        let _ = nats.create_stream(subject, stream_name);
                    }
                }
                drop(nats);
            }
            Some(&"CREATE_CONSUMER") => {
                if let Some(stream_name) = splitted_line.get(1) {
                    if let Some(consumer_name) = splitted_line.get(2) {
                        if let Some(delivery_subject) = splitted_line.get(3) {
                            let nats_ref_clone = Arc::clone(&nats_ref);
                            let _ = nats.create_and_consume(
                                stream_name,
                                consumer_name,
                                delivery_subject,
                                Box::new(move |payload| {
                                    println!("Payload: {}", payload.0);
                                    if let Ok(mut nats) = nats_ref_clone.lock() {
                                        if let Some(reply_to) = &payload.1 {
                                            let _ = nats.publish(reply_to, Some("Respuesta"), None);
                                        }
                                    }
                                }),
                            );
                        }
                    }
                }
                drop(nats);
            }
            Some(_) => {
                drop(nats);
                let msg = format!("{}\r\n", line.trim());
                println!("Enviando mensaje: |{}|", msg);
                let _ = writer.write_all(msg.as_bytes());
            }
            None => {
                drop(nats);
                println!("Error al leer la operación");
            }
        }
    }

    Ok(())
}
