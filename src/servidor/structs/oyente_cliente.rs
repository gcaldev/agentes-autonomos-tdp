use std::{
    io::{ErrorKind, Read},
    sync::{mpsc::Sender, Arc, RwLock},
};

use icwt_crypto::icwt_crypto::crypto::decrypt;

use crate::enums::errores_server::ErroresServer;

/// Estructura para leer lo que el cliente envia y pasarselo a traves de un channel
/// al cmd_exe. Ademas de un channel con el thread Ping por el que se le envian los Pong
/// recibidos por el servidor.
/// "listening_lock" es una flag que comparte con "Ping" para cerrar ambos threads ante errores.
pub struct OyenteCliente<R: Read + Send> {
    pub _adress_cliente: std::net::SocketAddr,
    pub array_bytes: R,
    notify_message: Sender<(String, usize)>,
    connection_id: usize,
    pong_notifier: Sender<usize>,
    listening_lock: Arc<RwLock<bool>>,
}

impl<R: Read + Send> OyenteCliente<R> {
    pub fn new(
        adress: std::net::SocketAddr,
        tuberia: R,
        notify_message: Sender<(String, usize)>,
        pong_notifier: Sender<usize>,
        listening_lock: Arc<RwLock<bool>>,
        connection_id: usize,
    ) -> OyenteCliente<R> {
        OyenteCliente {
            _adress_cliente: adress,
            array_bytes: tuberia,
            notify_message,
            connection_id,
            pong_notifier,
            listening_lock,
        }
    }

    /// Escucha los mensajes que el cliente le escribe al servidor.
    /// Filtra los mensajes que son privados o de uso interno del servidor
    /// Y ante desconexiones o errores, avisa al cmd_exe y al cliente. Y de tener que
    /// cerrarse lo hace de forma segura.
    pub fn escuchar_mensajes(&mut self) -> Result<(), ErroresServer> {
        loop {
            match escuchar_mensaje_2(&mut self.array_bytes, self.connection_id) {
                Ok(mensaje) => {
                    let command: Vec<&str> = mensaje.split_whitespace().collect();
                    if !command.is_empty() && command[0] == "PONG" {
                        self.pong_notifier.send(1).map_err(|_| {
                            ErroresServer::InternalError("Error de envío de mensaje".to_string())
                        })?;
                    } else if !command.is_empty()
                        && command[0] != "SEND_PING"
                        && command[0] != "SHUTDOWN"
                    {
                        //filtro para que no nos toquen cosas que no queremos
                        println!("mando {}", mensaje);
                        self.notify_message
                            .send((mensaje, self.connection_id))
                            .map_err(|_| {
                                ErroresServer::InternalError(
                                    "Error de envío de mensaje".to_string(),
                                )
                            })?;
                    }
                }
                Err(e) => {
                    match self.listening_lock.read() {
                        Ok(dejar_de_leer) => {
                            if *dejar_de_leer {
                                break; // si llega aca es porque debe dejar de leer
                            } else {
                                drop(dejar_de_leer);
                                match e {
                                    ErroresServer::ClientDisconnected(_) => {
                                        println!("{}", e);

                                        match self.listening_lock.write() {
                                            Ok(mut lock) => {
                                                *lock = true;
                                                drop(lock);
                                                self.notify_message
                                                    .send((
                                                        String::from("SHUTDOWN"),
                                                        self.connection_id,
                                                    ))
                                                    .map_err(|_| {
                                                        ErroresServer::InternalError(
                                                            "Error de envío de mensaje".to_string(),
                                                        )
                                                    })?;
                                            }
                                            Err(e) => {
                                                Err(e).map_err(|_| {
                                                    ErroresServer::InternalError(
                                                        "Error lock envenenado".to_string(),
                                                    )
                                                })?;
                                            }
                                        };

                                        print!("break en oyente");
                                        break;
                                    }
                                    _ => {
                                        let mensaje_error = format!("-ERR {}", e);
                                        //println!("Mando un error desde oyente");
                                        self.notify_message
                                            .send((mensaje_error, self.connection_id))
                                            .map_err(|_| {
                                                ErroresServer::InternalError(
                                                    "Error de envío de mensaje".to_string(),
                                                )
                                            })?;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            Err(e).map_err(|_| {
                                ErroresServer::InternalError("Error lock envenenado".to_string())
                            })?;
                        }
                    }
                }
            }
        }
        println!(
            "Se cierra la comunicacion con el cliente: {}",
            self.connection_id
        );
        Ok(())
    }
}

/// Atomiza los mensajes que arranquen con PUB, dejando todo en un mismo mensaje.
fn escuchar_mensaje_2(
    array_bytes_read: &mut dyn Read,
    client_id: usize,
) -> Result<String, ErroresServer> {
    let msg = escuchar_mensaje(array_bytes_read, client_id)?;
    if msg.starts_with("PUB") {
        let payload = escuchar_mensaje(array_bytes_read, client_id)?;
        Ok(format!("{}\r\n{}\r\n", msg, payload))
    } else {
        Ok(msg)
    }
}

/// Escucha el mensaje que le envia el cliente. Escucha hasta el primer \r\n que le llegue y cuando lo lee devuelve lo leido hasta el momento
fn escuchar_mensaje(
    array_bytes_read: &mut dyn Read,
    client_id: usize,
) -> Result<String, ErroresServer> {
    let mut buffer = Vec::new();
    loop {
        let mut byte = [0; 1];
        match array_bytes_read.read_exact(&mut byte) {
            Ok(_) => {
                buffer.push(byte[0]);
                if buffer.len() >= 2 && buffer[buffer.len() - 2..] == [b'\r', b'\n'] {
                    let decrypted_data = decrypt(&buffer[..buffer.len() - 2])
                        .map_err(|_| ErroresServer::ParserError)?;
                    let mensaje_str = String::from_utf8_lossy(&decrypted_data);
                    let mensaje_string = mensaje_str.into_owned();
                    return Ok(mensaje_string);
                }
            }
            Err(e)
                if (e.kind() == ErrorKind::ConnectionAborted)
                    || (e.kind() == ErrorKind::UnexpectedEof) =>
            {
                println!("Other side disconnected, {:?}", e);
                return Err(ErroresServer::ClientDisconnected(client_id.to_string()));
            }
            Err(e) => {
                println!("El error: {:?}", e);
                return Err(ErroresServer::ParserError);
            }
        }
    }
}

#[cfg(test)]
mod test {
    use std::{
        io::Write,
        net::{TcpListener, TcpStream},
        sync::{
            mpsc::{self, TryRecvError},
            Arc, RwLock,
        },
        thread::{self, sleep},
        time::Duration,
    };

    use icwt_crypto::icwt_crypto::crypto::encrypted_with_crlf;

    use crate::enums::errores_server::ErroresServer;

    use super::{escuchar_mensaje, OyenteCliente};

    fn write_msg<W: Write>(msg: &str, stream: &mut W) -> Result<(), ErroresServer> {
        let encrypted_data = encrypted_with_crlf(msg).map_err(|_| ErroresServer::ParserError)?;
        stream.write_all(&encrypted_data).map_err(|_| {
            ErroresServer::InternalError("Error al escribir el mensaje".to_string())
        })?;
        Ok(())
    }

    #[test]
    pub fn escucha_hasta_el_primer_barra_r_barra_n() {
        let msg = encrypted_with_crlf("hola que tal?\r\n").unwrap();
        let resultado1 = escuchar_mensaje(&mut msg.as_slice(), 0).unwrap();

        assert_eq!("hola que tal?".to_string(), resultado1);
    }

    #[test]
    pub fn escucha_hasta_el_primer_barra_r_barra_n_aun_habiendo_otro_despues() {
        let msg = encrypted_with_crlf("hola que tal?\r\n Bien y vos?\r\n").unwrap();
        let resultado1 = escuchar_mensaje(&mut msg.as_slice(), 0).unwrap();

        assert_eq!("hola que tal?".to_string(), resultado1);
    }

    #[test]
    pub fn si_llamo_dos_veces_escucha_el_segundo_barra_r_barra_n() {
        let binding = encrypted_with_crlf("hola que tal?\r\n Bien y vos?\r\n").unwrap();
        let mut msg = binding.as_slice();
        let _ = escuchar_mensaje(&mut msg, 0).unwrap();
        let resultado2 = escuchar_mensaje(&mut msg, 0).unwrap();

        assert_eq!(" Bien y vos?".to_string(), resultado2);
    }

    #[test]
    pub fn si_solo_esta_barra_r_barra_n_lee_vacio() {
        let msg = encrypted_with_crlf("\r\n").unwrap();
        let resultado1 = escuchar_mensaje(&mut msg.as_slice(), 0).unwrap();

        assert_eq!("".to_string(), resultado1);
    }

    #[test]
    pub fn leer_cuando_no_hay_nada_resulta_en_error() {
        let leer1 = "\r\n";
        let mut leer_bytes1 = leer1.as_bytes();
        let _ = escuchar_mensaje(&mut leer_bytes1, 0);
        let resultado2 = escuchar_mensaje(&mut leer_bytes1, 0);

        assert!(matches!(
            resultado2,
            Err(ErroresServer::ClientDisconnected(_))
        ));
    }

    #[test]
    pub fn leer_mensaje_sin_barra_r_barra_n_causa_desconexion() {
        let leer1 = "hola como estas";
        let mut leer_bytes1 = leer1.as_bytes();
        let resultado1 = escuchar_mensaje(&mut leer_bytes1, 0);
        let err = resultado1.unwrap_err();

        assert!(matches!(err, ErroresServer::ClientDisconnected(_)));
    }

    #[test]
    pub fn leer_cuando_hay_ademas_de_barra_r_barra_n_un_barra_n_suelto_en_el_medio() {
        let msg = encrypted_with_crlf("hola como\n estas\r\n").unwrap();
        let resultado1 = escuchar_mensaje(&mut msg.as_slice(), 0).unwrap();

        assert_eq!("hola como\n estas", resultado1);
    }

    #[test]
    pub fn leer_cuando_hay_ademas_de_barra_r_barra_n_mas_de_un_barra_n_suelto_en_el_medio() {
        let msg = encrypted_with_crlf("hola\n como\n estas\n ?\n Bien\n y\n vos?\r\n").unwrap();
        let resultado1 = escuchar_mensaje(&mut msg.as_slice(), 0).unwrap();

        assert_eq!("hola\n como\n estas\n ?\n Bien\n y\n vos?", resultado1);
    }

    #[test]
    pub fn leer_cuando_hay_antes_del_barra_r_barra_n_un_barra_n() {
        //let leer1 = "hola como estas\n\r\n";
        let msg = encrypted_with_crlf("hola como estas\n\r\n").unwrap();
        let resultado1 = escuchar_mensaje(&mut msg.as_slice(), 0).unwrap();

        assert_eq!("hola como estas\n", resultado1);
    }

    #[test]
    pub fn deja_de_escuchar_cuando_se_le_indica() {
        let (cmd_notifier, _cmd_receiver) = mpsc::channel::<(String, usize)>();
        let (pong_notifier, _pong_receiver) = mpsc::channel::<usize>();
        let listening_lock: Arc<RwLock<bool>> = Arc::new(RwLock::new(false));
        let lock_server = listening_lock.clone();
        let lock_cliente = listening_lock.clone();

        let se_cierra_escuchar: Arc<RwLock<bool>> = Arc::new(RwLock::new(false));
        let se_cierra_escuchar_copy = se_cierra_escuchar.clone();

        let builder = thread::Builder::new().name(format!("server thread-{}", 0));

        let server = builder
            .spawn(move || {
                let listener = TcpListener::bind("127.0.0.1:4246").unwrap();
                let (stream_th, socket_address_th) = listener.accept().unwrap();
                let mut oyente_cliente = OyenteCliente::new(
                    socket_address_th,
                    stream_th,
                    cmd_notifier,
                    pong_notifier,
                    lock_server,
                    0,
                );
                match oyente_cliente.escuchar_mensajes() {
                    Ok(_e) => {
                        let mut se_cierra_escuchar_copy_lock =
                            se_cierra_escuchar_copy.write().unwrap();
                        *se_cierra_escuchar_copy_lock = true;
                    }
                    _ => (),
                }
                drop(listener);
            })
            .unwrap();

        let builder = thread::Builder::new().name(format!("client thread-{}", 0));

        let cliente = builder
            .spawn(move || {
                sleep(Duration::from_millis(10));
                println!("cedio ejecucion");
                let _ = TcpStream::connect("127.0.0.1:4246").unwrap();
                println!("se conecta al server");
                let mut lock = lock_cliente.write().unwrap(); // le digo a escuchar mesaje que deje de hacerlo.
                println!("lockea la flag");
                *lock = true;
                println!("pasa a true el lock y termina el thread cliente");
            })
            .unwrap();

        server.join().unwrap();
        cliente.join().unwrap();
        let lock = se_cierra_escuchar.read().unwrap();
        assert!(*lock);
    }

    #[test]
    pub fn si_el_mensaje_es_pong_lo_catchea_antes_y_lo_manda_a_ping() {
        let (cmd_notifier, _cmd_receiver) = mpsc::channel::<(String, usize)>();
        let (pong_notifier, pong_receiver) = mpsc::channel::<usize>();
        let listening_lock: Arc<RwLock<bool>> = Arc::new(RwLock::new(false));
        let lock_server = listening_lock.clone();
        let lock_cliente = listening_lock.clone();

        let builder = thread::Builder::new().name(format!("server thread-{}", 0));

        let server = builder
            .spawn(move || {
                let listener = TcpListener::bind("127.0.0.1:4250").unwrap();
                let (stream_th, socket_address_th) = listener.accept().unwrap();
                let mut oyente_cliente = OyenteCliente::new(
                    socket_address_th,
                    stream_th,
                    cmd_notifier,
                    pong_notifier,
                    lock_server,
                    0,
                );
                oyente_cliente.escuchar_mensajes().unwrap();
                drop(listener);
            })
            .unwrap();

        let builder = thread::Builder::new().name(format!("client thread-{}", 0));

        let cliente = builder
            .spawn(move || {
                sleep(Duration::from_millis(10));
                let mut tcp_stream = TcpStream::connect("127.0.0.1:4250").unwrap();
                write_msg("PONG\r\n", &mut tcp_stream).unwrap();
                let mut lock = lock_cliente.write().unwrap();
                *lock = true;
            })
            .unwrap();

        server.join().unwrap();
        cliente.join().unwrap();

        let result = pong_receiver.recv().unwrap();
        assert_eq!(1, result);
    }

    #[test]
    pub fn si_lo_mandado_por_el_cliente_son_comandos_de_uso_interno_entonces_no_pasa_el_filtro() {
        let (cmd_notifier, cmd_receiver) = mpsc::channel::<(String, usize)>();
        let (pong_notifier, _pong_receiver) = mpsc::channel::<usize>();
        let listening_lock: Arc<RwLock<bool>> = Arc::new(RwLock::new(false));
        let lock_server = listening_lock.clone();
        let lock_cliente = listening_lock.clone();

        let blockeo_send_ping: Arc<RwLock<bool>> = Arc::new(RwLock::new(false));
        let blockeo_send_ping_copy = blockeo_send_ping.clone();

        let blockeo_shutdown: Arc<RwLock<bool>> = Arc::new(RwLock::new(false));
        let blockeo_shutdown_copy = blockeo_shutdown.clone();

        let builder = thread::Builder::new().name(format!("server thread-{}", 0));

        let server = builder
            .spawn(move || {
                let listener = TcpListener::bind("127.0.0.1:4242").unwrap();
                let (stream_th, socket_address_th) = listener.accept().unwrap();
                let mut oyente_cliente = OyenteCliente::new(
                    socket_address_th,
                    stream_th,
                    cmd_notifier,
                    pong_notifier,
                    lock_server,
                    0,
                );
                oyente_cliente.escuchar_mensajes().unwrap();
                drop(listener);
            })
            .unwrap();

        let builder = thread::Builder::new().name(format!("client thread-{}", 0));

        let cliente = builder
            .spawn(move || {
                sleep(Duration::from_millis(10));
                let mut tcp_stream = TcpStream::connect("127.0.0.1:4242").unwrap();
                write_msg("SEND_PING\r\n", &mut tcp_stream).unwrap();
                sleep(Duration::from_millis(10));
                match cmd_receiver.try_recv() {
                    Err(TryRecvError::Empty) => {
                        let mut lock = blockeo_send_ping_copy.write().unwrap();
                        *lock = true;
                    }
                    _ => (),
                }
                write_msg("SHUTDOWN\r\n", &mut tcp_stream).unwrap();
                sleep(Duration::from_millis(10));
                match cmd_receiver.try_recv() {
                    Err(TryRecvError::Empty) => {
                        let mut lock = blockeo_shutdown_copy.write().unwrap();
                        *lock = true;
                    }
                    _ => (),
                }

                let mut lock = lock_cliente.write().unwrap();
                *lock = true;
            })
            .unwrap();

        server.join().unwrap();
        cliente.join().unwrap();

        assert!(*blockeo_send_ping.read().unwrap() && *blockeo_shutdown.read().unwrap());
    }

    #[test]
    pub fn el_cliente_se_comunica_correctamente_mediante_el_uso_del_protocolo_y_comandos_publicos()
    {
        let (cmd_notifier, cmd_receiver) = mpsc::channel::<(String, usize)>();
        let (pong_notifier, _pong_receiver) = mpsc::channel::<usize>();
        let listening_lock: Arc<RwLock<bool>> = Arc::new(RwLock::new(false));
        let lock_server = listening_lock.clone();
        let lock_cliente = listening_lock.clone();

        let llego_ping: Arc<RwLock<bool>> = Arc::new(RwLock::new(false));
        let llego_ping_copy = llego_ping.clone();

        let llego_pub: Arc<RwLock<bool>> = Arc::new(RwLock::new(false));
        let llego_pub_copy = llego_pub.clone();

        let llego_sub: Arc<RwLock<bool>> = Arc::new(RwLock::new(false));
        let llego_sub_copy = llego_sub.clone();

        let builder = thread::Builder::new().name(format!("server thread-{}", 0));

        let server = builder
            .spawn(move || {
                let listener = TcpListener::bind("127.0.0.1:8080").unwrap();
                let (stream_th, socket_address_th) = listener.accept().unwrap();
                let mut oyente_cliente = OyenteCliente::new(
                    socket_address_th,
                    stream_th,
                    cmd_notifier,
                    pong_notifier,
                    lock_server,
                    0,
                );
                oyente_cliente.escuchar_mensajes().unwrap();
                drop(listener);
            })
            .unwrap();

        let builder = thread::Builder::new().name(format!("client thread-{}", 0));

        let cliente = builder
            .spawn(move || {
                sleep(Duration::from_millis(10));
                let mut tcp_stream = TcpStream::connect("127.0.0.1:8080").unwrap();
                write_msg("PING\r\n", &mut tcp_stream).unwrap();
                match cmd_receiver.recv() {
                    Ok(msg) => {
                        let mut lock = llego_ping_copy.write().unwrap();
                        *lock = msg.0 == "PING";
                    }
                    _ => (),
                }
                write_msg("SUB time 0\r\n", &mut tcp_stream).unwrap();
                match cmd_receiver.recv() {
                    Ok(msg) => {
                        let mut lock = llego_sub_copy.write().unwrap();
                        *lock = msg.0 == "SUB time 0";
                    }
                    _ => (),
                }
                write_msg("PUB time 11\r\nhola manola\r\n", &mut tcp_stream).unwrap();
                match cmd_receiver.recv() {
                    Ok(msg) => {
                        let mut lock = llego_pub_copy.write().unwrap();
                        println!("EL msg es: {}", msg.0);
                        *lock = msg.0 == "PUB time 11\r\nhola manola\r\n";
                    }
                    _ => (),
                }
                let mut lock = lock_cliente.write().unwrap();
                *lock = true;
            })
            .unwrap();

        server.join().unwrap();
        cliente.join().unwrap();

        assert!(
            *llego_ping.read().unwrap() && *llego_sub.read().unwrap() && *llego_pub.read().unwrap()
        );
    }
}
