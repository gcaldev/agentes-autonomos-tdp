use super::in_memory_repository::InMemoryUsersRepository;
use crate::enums::errores_server::ErroresServer;
use crate::structs::command_executor::CommandExecutor;
use crate::structs::oyente_cliente::OyenteCliente;
use crate::structs::ping::Ping;
use std::collections::HashMap;
use std::io::Write;
use std::net::TcpListener;
use std::sync::mpsc::{self};
use std::sync::RwLock;
use std::sync::{Arc, Mutex};
use std::thread::{self, sleep};
use std::time::Duration;

use icwt_crypto::icwt_crypto::crypto::encrypted_with_crlf;
use logger::logger::{LogType, Logger};

const AUTH_REQUIRED: bool = true;
const WAIT_TIME_PONG: usize = 30; //segs
const TIME_BETWEEN_PINGS: usize = 50; //segs. 300segs == 5 min

/// Estructura para getionar las conexiones de cada cliente
pub struct Server<'a> {
    address: &'a str,
    connection_id_counter: usize,
    max_connections: usize,
    count_connections: usize,
    logger: Arc<RwLock<Logger>>,
}

//Server implementa metodos que no son publicos, son para uso propio.
impl Server<'_> {
    pub fn new(address: &str) -> Server {
        Server {
            address,
            connection_id_counter: 1,
            max_connections: 15,
            count_connections: 0,
            logger: Arc::new(RwLock::new(Logger::new("server_log.txt"))),
        }
    }

    /// Corre el servidor y esta constantemente atento a nuevas conexiones entrantes
    /// Siempre que se lo llama y por unica vez crea el thread de cmd_exe.
    /// Por cada cliente crea el thread OyenteCliente y Ping.
    pub fn run(&mut self) -> std::io::Result<()> {
        let listener = TcpListener::bind(self.address)?;
        let mut handlers = Vec::new();

        let builder = thread::Builder::new().name("command-executor".to_string());

        let clients_map = Arc::new(Mutex::new(HashMap::new()));

        let (cmd_notifier, cmd_receiver) = mpsc::channel::<(String, usize)>();

        let cmd_clients_map = clients_map.clone();

        let address = self.address.to_string();

        let log_cmd = self.logger.clone();

        let clients_logs_map = Arc::new(RwLock::new(HashMap::new()));
        let logs_clone = clients_logs_map;
        let internal_cmd_notifier = cmd_notifier.clone();

        let cmd_exec_handler: thread::JoinHandle<Result<(), ErroresServer>> =
            builder.spawn(move || {
                let mut command_executor = CommandExecutor::new(
                    &address,
                    cmd_receiver,
                    internal_cmd_notifier,
                    cmd_clients_map,
                    AUTH_REQUIRED,
                    InMemoryUsersRepository::new(),
                    logs_clone,
                );

                loop {
                    //de momento esto corta solo por terminal.
                    if let Err(e) = command_executor.ejecutar_comando() {
                        println!("Error al ejecutar comando: {}", e);
                        match log_cmd.write() {
                            Ok(log_lock) => {
                                log_lock.log(
                                    LogType::Error,
                                    format!("Error durante la ejecucion del cmd_exe: {}", e)
                                        .as_str(),
                                );
                            }
                            Err(error) => {
                                println!("Saliendo del cmd_exe: {}", error);
                                return Err(error).map_err(|_| {
                                    ErroresServer::InternalError("Lock envenenado".to_string())
                                });
                            }
                        }
                    }
                }
            })?;

        for mut stream in listener.incoming().flatten() {
            let stream_th = stream.try_clone()?;
            let socket_address_th = stream.peer_addr()?;

            if self.count_connections >= self.max_connections {
                let error = format!("-ERR {}\r\n", ErroresServer::MaximumConnectionsExceeded);

                self.logger
                    .write()
                    .map_err(|_| ErroresServer::InternalError("Lock envenenado".to_string()))?
                    .log(LogType::Error, error.as_str());

                write_msg(&error, &mut stream)?;
                continue;
            }

            let mensaje = format!(
                "Nueva conexión establecida con cliente de socket adress: {:?}",
                stream.peer_addr()
            );
            println!("{}", mensaje);

            self.logger
                .write()
                .map_err(|_| ErroresServer::InternalError("Lock envenenado".to_string()))?
                .log(LogType::Info, mensaje.as_str());

            let builder =
                thread::Builder::new().name(format!("thread-{}", self.connection_id_counter));
            let cmd_notifier_th = mpsc::Sender::clone(&cmd_notifier);

            let (pong_notifier, pong_receiver) = mpsc::channel::<usize>();
            let id = self.connection_id_counter;
            let listening_lock: Arc<RwLock<bool>> = Arc::new(RwLock::new(false));
            let lock_lectura = listening_lock.clone();
            let clients = clients_map.clone();

            let id_connection_lock = Arc::new(Mutex::new(self.connection_id_counter));
            let id_connections_clone = id_connection_lock.clone();

            let client_connection = builder.spawn(move || {
                let mut oyente_cliente = OyenteCliente::new(
                    socket_address_th,
                    stream_th,
                    cmd_notifier_th,
                    pong_notifier,
                    lock_lectura,
                    id,
                );

                if let Err(e) = oyente_cliente.escuchar_mensajes() {
                    println!("Error: {}", e);
                }
                quitar_cliente(clients, id);
                match id_connections_clone.lock() {
                    Ok(mut id) => {
                        *id -= 1;
                    }
                    Err(_) => {
                        println!("Lock envenenado");
                    }
                }
            })?;

            let builder =
                thread::Builder::new().name(format!("ping thread-{}", self.connection_id_counter));
            let client_id = self.connection_id_counter;
            let cmd_notifier_ping = mpsc::Sender::clone(&cmd_notifier);

            let ping = builder.spawn(move || {
                sleep(Duration::from_secs(5)); //para darle tiempo a que todo este correcto
                let mut ping = Ping::new(
                    pong_receiver,
                    cmd_notifier_ping,
                    listening_lock,
                    WAIT_TIME_PONG,
                    TIME_BETWEEN_PINGS,
                    client_id,
                );
                if let Err(e) = ping.mantain_connection() {
                    e.to_string();
                }
                println!("salgo");
            })?;

            self.info(&mut stream)?;

            handlers.push((client_connection, ping));

            if let Ok(mut streams) = clients_map.lock() {
                streams.insert(self.connection_id_counter, stream);
                self.connection_id_counter += 1;
                self.count_connections += 1;
            }
        }

        for handler in handlers {
            if handler.0.join().is_err() {
                self.log(LogType::Warn, "Error al cerrar el oyente cliente");
            }
            if handler.1.join().is_err() {
                self.log(LogType::Warn, "Error al cerrar el ping");
            }
        }

        let _ = cmd_exec_handler.join().map_err(|_| {
            ErroresServer::InternalError("Error producido en el cmd_exe".to_string())
        })?;
        Ok(())
    }

    /// Hace log de los errores que ocurre o acciones que se desean guardar
    fn log(&self, log_type: LogType, msg: &str) {
        if let Ok(log_lock) = self.logger.write() {
            log_lock.log(log_type, msg);
        }
    }

    /// Le manda un mensaje INFO a cada nuevo cliente
    pub fn info(&self, mut stream: &mut dyn Write) -> Result<(), ErroresServer> {
        let port: Vec<&str> = self.address.split(':').collect();

        if port.len() < 2 {
            return Err(ErroresServer::InternalError("Puerto Invalido".to_string()));
        }

        let info_json_part_1 = "{\"server_id\":1,\"server_name\":\"in_code_we_trust\",\"version\":\"2.6.5\"},\"go\":\"go1.16.10\"},\"host\":\"0.0.0.0\",\"port\":\"".to_string();
        let info_json = info_json_part_1
            + port[1]
            + "\",\"auth_required\":"
            + &AUTH_REQUIRED.to_string()
            + "\",\"headers\":false,\"max_payload\":1048576,\"proto\":\"1.2.0\"}";
        let msg_info = format!("INFO {}\r\n", info_json);

        write_msg(&msg_info, &mut stream)?;

        println!("Enviado: {:?}", msg_info);
        Ok(())
    }
}

fn write_msg<W: Write>(msg: &str, stream: &mut W) -> Result<(), ErroresServer> {
    let encrypted_data = encrypted_with_crlf(msg).map_err(|_| ErroresServer::ParserError)?;
    stream
        .write_all(&encrypted_data)
        .map_err(|_| ErroresServer::InternalError("Error al escribir el mensaje".to_string()))?;
    Ok(())
}

/// Quita un cliente que ya no esta mas en el server, porque se desconecto
fn quitar_cliente<W: Write + Send>(hashmap: Arc<Mutex<HashMap<usize, W>>>, id: usize) {
    if let Ok(mut hash_lock) = hashmap.lock() {
        println!("El hashmap: {:?}", hash_lock.keys());
        hash_lock.remove(&id);
        println!("El hashmap: {:?}", hash_lock.keys());
    }
}

#[cfg(test)]
mod test_command_executor_login {
    use icwt_crypto::icwt_crypto::crypto::decrypt_with_crlf;
    use logger::logger::Logger;

    use crate::enums::errores_server::ErroresServer;
    use crate::structs::user::User;
    use crate::structs::users_repository::UsersRepository;
    use std::str;
    use std::sync::RwLock;
    use std::{
        collections::HashMap,
        io::{self, Read, Write},
        sync::{
            mpsc::{self, Sender},
            Arc, Mutex,
        },
        time::Duration,
    };

    use super::CommandExecutor;

    /// MockTcpStream es una mock que implementa los traits Read y Write, los mismos que implementa el TcpStream
    struct MockUsersRepository {}

    impl MockUsersRepository {
        fn new() -> Self {
            MockUsersRepository {}
        }
    }

    impl UsersRepository for MockUsersRepository {
        fn find_user_by_username_and_password(
            &self,
            username: &str,
            password: &str,
        ) -> Option<User> {
            if username == "admin" && password == "admin" {
                Some(User::new(
                    "admin".to_string(),
                    "admin".to_string(),
                    vec!["time.>".to_string()],
                    vec!["otro".to_string()],
                    "userMock_log.txt",
                ))
            } else {
                None
            }
        }
    }

    struct MockTcpStream {
        read_data: Vec<u8>,
        write_data: Vec<u8>,
        sender: Sender<String>,
    }

    impl MockTcpStream {
        #[allow(dead_code)]
        fn new(sender: Sender<String>) -> Self {
            MockTcpStream {
                read_data: Vec::new(),
                write_data: Vec::new(),
                sender,
            }
        }
    }

    impl Read for MockTcpStream {
        /// Lee bytes del stream hasta completar el buffer y devuelve cuantos bytes fueron leidos
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.read_data.as_slice().read(buf)
        }
    }

    impl Write for MockTcpStream {
        /// Escribe el valor del buffer en el stream y devuelve cuantos bytes fueron escritos
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let msg = decrypt_with_crlf(buf).unwrap();
            self.sender.send(msg.to_owned()).unwrap();
            self.write_data.write(msg.as_bytes()).unwrap();
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.write_data.flush()
        }
    }

    struct BeforeEachSetup {
        cmd_notifier: mpsc::Sender<(String, usize)>,
        cmd_receiver: mpsc::Receiver<(String, usize)>,
        clientes_map: Arc<Mutex<HashMap<usize, MockTcpStream>>>,
        clients_logs_map: Arc<RwLock<HashMap<usize, Logger>>>,
        repo: MockUsersRepository,
        get_written_data: Box<dyn Fn() -> String>,
    }

    fn before_each() -> BeforeEachSetup {
        let (sender, receiver) = mpsc::channel::<String>();
        let (cmd_notifier, cmd_receiver) = mpsc::channel::<(String, usize)>();
        let client_connection = MockTcpStream {
            read_data: Vec::new(),
            write_data: Vec::new(),
            sender,
        };

        let mut hashmap: HashMap<usize, MockTcpStream> = HashMap::new();
        hashmap.insert(1, client_connection);
        let clientes_map = Arc::new(Mutex::new(hashmap));
        let repo = MockUsersRepository::new();

        let get_written_data = move || -> String {
            let res = receiver.recv_timeout(Duration::from_secs(3));

            res.unwrap()
        };

        BeforeEachSetup {
            cmd_notifier,
            cmd_receiver,
            clientes_map,
            clients_logs_map: Arc::new(RwLock::new(HashMap::new())),
            repo,
            get_written_data: Box::new(get_written_data),
        }
    }

    #[test]
    fn test01_connect_sin_login_ok() {
        let setup = before_each();
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier,
            setup.clientes_map,
            false,
            setup.repo,
            setup.clients_logs_map,
        );

        command_executor.connect(&"{}", 1).unwrap();

        assert_eq!("+OK\r\n", (setup.get_written_data)());
    }

    #[test]
    fn test02_connect_con_login_ok() {
        let setup = before_each();
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier,
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", 1)
            .unwrap();

        assert_eq!("+OK\r\n", (setup.get_written_data)());
    }

    #[test]
    fn test03_connect_con_login_credenciales_invalidas_error() {
        let setup = before_each();
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier,
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let res = command_executor.connect(&"{\"user\":\"admin\",\"pass\":\"lapassestamal\"}", 1);

        assert!(matches!(
            res.unwrap_err(),
            ErroresServer::AuthorizationViolation
        ));
        assert_eq!(
            "-ERR Authorization Violation\r\n",
            (setup.get_written_data)()
        );
    }

    #[test]
    fn test04_suscribirse_sin_login_ok() {
        let setup = before_each();
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier,
            setup.clientes_map,
            false,
            setup.repo,
            setup.clients_logs_map,
        );
        command_executor.connect(&"{}", 1).unwrap();
        (setup.get_written_data)();

        command_executor.sub(&"time.* 1", 1).unwrap();

        assert_eq!("+OK\r\n", (setup.get_written_data)());
    }

    #[test]
    fn test05_suscribirse_con_login_ok() {
        let setup = before_each();
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier,
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", 1)
            .unwrap();
        (setup.get_written_data)();

        command_executor.sub(&"time.us 1", 1).unwrap();

        assert_eq!("+OK\r\n", (setup.get_written_data)());
    }

    #[test]
    fn test06_suscribirse_a_subject_no_autorizado() {
        let setup = before_each();
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier,
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", 1)
            .unwrap();
        (setup.get_written_data)();

        command_executor.sub(&"area.us 1", 1).unwrap();

        assert_eq!(
            "-ERR Permissions Violation for Subscription to area.us\r\n",
            (setup.get_written_data)()
        );
    }

    #[test]
    fn test07_publicar_sin_login_ok() {
        let setup = before_each();
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            false,
            setup.repo,
            setup.clients_logs_map,
        );
        command_executor.connect(&"{}", 1).unwrap();
        (setup.get_written_data)();

        command_executor
            .publish(&"area.us 17\r\nmensaje_de_prueba\r\n", 1)
            .unwrap();

        assert_eq!("+OK\r\n", (setup.get_written_data)());
    }

    #[test]
    fn test08_publicar_a_subject_no_autorizado() {
        let setup = before_each();
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", 1)
            .unwrap();
        (setup.get_written_data)();

        command_executor
            .publish(&"area.us 17\r\nmensaje_de_prueba\r\n", 1)
            .unwrap();

        assert_eq!(
            "-ERR Permissions Violation for Publish to area.us\r\n",
            (setup.get_written_data)()
        );
    }

    #[test]
    fn test09_publicar_con_login_ok() {
        let setup = before_each();
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", 1)
            .unwrap();
        (setup.get_written_data)();

        command_executor
            .publish(&"otro 17\r\nmensaje_de_prueba\r\n", 1)
            .unwrap();

        assert_eq!("+OK\r\n", (setup.get_written_data)());
    }
}
