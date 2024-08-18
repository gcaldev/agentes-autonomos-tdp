# Taller de Programacion

## Grupo

[Mateo Ezequiel Ibañez](https://github.com/MateoIbaniez) - 107983

[Gonzalo Calderon](https://github.com/gcaldev) - 107143

[Julian Mutchinick](https://github.com/julimuchi) - 99479

[Kevin Alberto Vallejo](https://github.com/Kevin00404) - 109975

## Como usar

Para poder levantar las aplicaciones alcanza con correr el server y las aplicaciones de UI correspondientes al sistema de monitoreo, dron, central de camaras.

En caso de querer levantar algun componente por separado para facilitar la prueba durante el desarrollo también se puede hacer.

Para levantar el servidor simplemente estando parado en el directorio raiz del proyecto y corriendo el comando alcanza. 

Si queremos ejecutar el resto de aplicaciones debemos dirigirnos a su directorio y ejecutar el comando que le corresponde.

### Servidor

Tener en cuenta que se debe borrar los archivos de la carpeta resources dentro de jetstream en caso de que estén.

```
cargo run --bin 0server <puerto>
```

### Sistema de monitoreo UI

```
cargo run
```

### Dron UI
```
cargo run
```

### Central de cámaras UI
```
cargo run
```

### Cliente
```
cargo run -- <ip_server> <puerto_server>
```

## Como testear

Para el desarrollo de las aplicaciones seguimos los lineamientos sugeridos al momento de realizar testing, por medio del uso de traits realizamos algunos mocks para llevar a cabo el testing unitario (como puede ser el caso del nats client mockeado o del mock del tcp stream en algunos tests del servidor). Mientras que en otras oportunidades donde vimos mas adecuado realizar las pruebas por medio de tests de integración para considerar la colaboración entre los distintos componentes (como puede ser el caso de jetstream y algunos tests del servidor).

Independientemente de si queremos correr tests unitarios o de integración, para poder lograrlo debemos dirigirnos al directorio de la aplicacion (en caso del servidor pararnos en el directorio raiz como se mencionó antes) y ejecutar el siguiente comando:

```
cargo test
```
