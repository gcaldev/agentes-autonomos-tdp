use std::thread::sleep;
use std::{
    sync::{
        mpsc::{Receiver, Sender},
        Arc, RwLock,
    },
    time::Duration,
};

use crate::enums::errores_server::ErroresServer;

/// Estructura para mantener viva la conexion. Se comunica con el cmd_exe a traves de un channel ("cmd_notifier").
/// Y recibe los Pongs a traves de "pong_receiver". Tambien tiene los tiempos para esperar un pong y para hacer un ping.
/// "listening_lock" es una flag que comparte con "OyenteCliente" para cerrar ambos threads ante errores.
pub struct Ping {
    pong_receiver: Receiver<usize>,
    waiting_time: usize,
    time_between_pings: usize,
    client_id: usize,
    listening_lock: Arc<RwLock<bool>>,
    cmd_notifier: Sender<(String, usize)>,
}

impl Ping {
    pub fn new(
        pong_receiver: Receiver<usize>,
        cmd_notifier: Sender<(String, usize)>,
        listening_lock: Arc<RwLock<bool>>,
        waiting_time: usize,
        time_between_pings: usize,
        client_id: usize,
    ) -> Self {
        Ping {
            pong_receiver,
            waiting_time,
            time_between_pings,
            client_id,
            listening_lock,
            cmd_notifier,
        }
    }

    /// Se encarga de mantener viva la conexion con el cliente, para ello cada "x" tiempo se le envia un PING
    /// si el cliente responde PONG entonces, se hace un sleep y despues todo vuelve a repetirse.
    /// Si el cliente no responde PONG dentro de un "x" tiempo entonces se corta la conexion con el
    /// Ante errores o desconexiones se avisa al cmd_exe y al cliente. Y de ser necesario el thread ping se
    /// cerrara correctamente
    pub fn mantain_connection(&mut self) -> Result<(), ErroresServer> {
        loop {
            //manda ping
            match self.listening_lock.read() {
                Ok(lock) => {
                    if !(*lock) {
                        drop(lock);
                        self.cmd_notifier
                            .send(("SEND_PING".to_string(), self.client_id))
                            .map_err(|_| {
                                ErroresServer::InternalError(
                                    "Error de envío de mensaje".to_string(),
                                )
                            })?;

                        match self
                            .pong_receiver
                            .recv_timeout(Duration::from_secs(self.waiting_time as u64))
                        {
                            Ok(_) => {
                                println!("server recibe PONG");
                                sleep(Duration::new(self.time_between_pings as u64, 0));
                                continue;
                            }
                            Err(_) => {
                                // si estoy aca es porque:
                                //  - se acabo el tiempo de espera para recibir un PONG de parte del cliente.
                                //  - se desconecto la otra punta.
                                //DESCONECTARME DE ESE CLIENTE:
                                match self.listening_lock.write() {
                                    Ok(mut dejar_de_escuchar) => {
                                        if !(*dejar_de_escuchar) {
                                            let mensaje_error =
                                                format!("-ERR {}", ErroresServer::StaleConnection);
                                            self.cmd_notifier
                                                .send((mensaje_error, self.client_id))
                                                .map_err(|_| {
                                                    ErroresServer::InternalError(
                                                        "Error de envío de mensaje".to_string(),
                                                    )
                                                })?;

                                            self.cmd_notifier
                                                .send(("SHUTDOWN".to_string(), self.client_id))
                                                .map_err(|_| {
                                                    ErroresServer::InternalError(
                                                        "Error de envío de mensaje".to_string(),
                                                    )
                                                })?;
                                            *dejar_de_escuchar = true;
                                        }
                                        println!(
                                            "Se cierra el hilo ping del cliente {}",
                                            self.client_id
                                        );
                                        break;
                                    }
                                    Err(e) => {
                                        Err(e).map_err(|_| {
                                            ErroresServer::InternalError(
                                                "Error lock envenenado".to_string(),
                                            )
                                        })?;
                                    }
                                }
                            }
                        }
                    } else {
                        println!("Se cierra el hilo ping del cliente {}", self.client_id);
                        break;
                    }
                }
                Err(e) => {
                    Err(e).map_err(|_| {
                        ErroresServer::InternalError("Error lock envenenado".to_string())
                    })?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use std::{
        sync::{mpsc, Arc, Mutex, RwLock},
        thread::{self, sleep},
        time::Duration,
    };

    use super::Ping;

    #[test]
    pub fn manda_send_ping() {
        let waiting_time = 1;
        let time_between_pings = 3;
        let client_id = 0;
        let (cmd_notifier, cmd_receiver) = mpsc::channel::<(String, usize)>();
        let (_pong_notifier, pong_receiver) = mpsc::channel::<usize>();
        let listening_lock: Arc<RwLock<bool>> = Arc::new(RwLock::new(false));

        let mut ping = Ping::new(
            pong_receiver,
            cmd_notifier,
            listening_lock,
            waiting_time,
            time_between_pings,
            client_id,
        );
        ping.mantain_connection().unwrap();
        let send_ping = cmd_receiver.recv().unwrap();
        assert!(send_ping.0 == "SEND_PING".to_owned());
    }

    #[test]
    pub fn si_es_interrumpido_antes_de_tiempo_vuelve_a_mandar_ping() {
        let waiting_time = 1;
        let time_between_pings = 3;
        let client_id = 0;
        let (cmd_notifier, cmd_receiver) = mpsc::channel::<(String, usize)>();
        let (pong_notifier, pong_receiver) = mpsc::channel::<usize>();
        let listening_lock: Arc<RwLock<bool>> = Arc::new(RwLock::new(false));

        let send_ping = Arc::new(Mutex::new(Vec::new()));
        let send_ping_th = send_ping.clone();

        let builder = thread::Builder::new().name(format!("thread-ping {}", client_id));

        let ping_th = builder
            .spawn(move || {
                let mut ping = Ping::new(
                    pong_receiver,
                    cmd_notifier,
                    listening_lock,
                    waiting_time,
                    time_between_pings,
                    client_id,
                );
                sleep(Duration::from_secs(1));
                let _ = ping.mantain_connection();
            })
            .unwrap();

        let builder = thread::Builder::new().name(format!("thread-{}", client_id));

        let responder_pong_a_ping = builder
            .spawn(move || {
                let mut send_ping_1 = send_ping_th.lock().unwrap();
                send_ping_1.push(cmd_receiver.recv().unwrap());
                pong_notifier.send(0).unwrap();
                send_ping_1.push(cmd_receiver.recv().unwrap());
            })
            .unwrap();

        ping_th.join().unwrap();
        responder_pong_a_ping.join().unwrap();

        let send_ping_1 = send_ping.lock().unwrap();
        println!("send_ping: {:?}", send_ping);
        assert!(
            send_ping_1.len() == 2
                && send_ping_1[0].0 == "SEND_PING"
                && send_ping_1[1].0 == "SEND_PING"
        );
    }

    #[test]
    pub fn si_no_es_interrumpido_antes_de_tiempo_manda_shutdown_y_err() {
        let waiting_time = 1;
        let time_between_pings = 3;
        let client_id = 0;
        let (cmd_notifier, cmd_receiver) = mpsc::channel::<(String, usize)>();
        let (_pong_notifier, pong_receiver) = mpsc::channel::<usize>();
        let listening_lock: Arc<RwLock<bool>> = Arc::new(RwLock::new(false));

        let send_ping = Arc::new(Mutex::new(Vec::new()));
        let send_ping_th = send_ping.clone();

        let builder = thread::Builder::new().name(format!("thread-ping {}", client_id));

        let ping_th = builder
            .spawn(move || {
                let mut ping = Ping::new(
                    pong_receiver,
                    cmd_notifier,
                    listening_lock,
                    waiting_time,
                    time_between_pings,
                    client_id,
                );
                sleep(Duration::from_secs(1));
                let _ = ping.mantain_connection();
            })
            .unwrap();

        let builder = thread::Builder::new().name(format!("thread-{}", client_id));

        let responder_pong_a_ping = builder
            .spawn(move || {
                let mut send_ping_1 = send_ping_th.lock().unwrap();
                send_ping_1.push(cmd_receiver.recv().unwrap());
                send_ping_1.push(cmd_receiver.recv().unwrap());
                send_ping_1.push(cmd_receiver.recv().unwrap());
            })
            .unwrap();

        ping_th.join().unwrap();
        responder_pong_a_ping.join().unwrap();

        let send_ping_1 = send_ping.lock().unwrap();
        println!("send_ping: {:?}", send_ping);
        assert!(
            send_ping_1.len() == 3
                && send_ping_1[0].0 == "SEND_PING"
                && send_ping_1[1].0 == "-ERR Stale connection"
                && send_ping_1[2].0 == "SHUTDOWN"
        );
    }
}
