use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Error, Write},
};

/// Lee un archivo CSV y retorna un vector con las lineas del archivo
pub fn read_csv(path: &str) -> Result<Vec<String>, Error> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut lines = Vec::new();
    for line in reader.lines() {
        lines.push(line?);
    }
    Ok(lines)
}

/// Actualiza el registro que matchee con `key` en el archivo CSV
pub fn update_csv_file(filename: &str, updated_data: String, key: String) -> Result<(), Error> {
    let mut lines: Vec<String> = Vec::new();
    let mut found = false;

    if let Ok(file_lines) = read_csv(filename) {
        for line in file_lines {
            let mut parts = line.split(',');
            if let Some(name) = parts.next() {
                if key == name.trim() {
                    lines.push(updated_data.to_string());
                    found = true;
                } else {
                    lines.push(line);
                }
            }
        }
    } else if let Err(e) = read_csv(filename) {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(e);
        }
    }

    if !found {
        lines.push(updated_data);
    }

    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(filename)?;

    for line in lines {
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
    }

    Ok(())
}
