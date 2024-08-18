use azure_vision_client::analyze_image;
use std::collections::HashSet;
use std::fs::File;
use std::io::{self, BufRead};

// ""

/// Lee los tags guardados en un csv
fn read_incident_tags() -> io::Result<HashSet<String>> {
    println!("Entre en el leer el csv con los tags");
    let file = File::open("../../detector_incidentes/src/incidentes.txt")?;
    println!("Tags abiertos correctamente");
    let reader = io::BufReader::new(file);
    let mut tags = HashSet::new();

    for line in reader.lines() {
        if let Ok(tag) = line {
            tags.insert(tag.trim().to_string());
        }
    }

    Ok(tags)
}

/// Si los tags devueltos por analyze_image tienen mas de 3 coincidencias con los tags propios, entonces devuelve True. Si no false
pub fn es_incidente(image_data: &[u8]) -> Result<bool, Box<dyn std::error::Error>> {
    println!("Entre en: \"es incidentes\"");
    let tags = analyze_image(image_data)?; //tags es el arra de tags

    let incident_tags = read_incident_tags()?;
    let mut match_count = 0;

    for tag in tags {
        if incident_tags.contains(&tag) {
            match_count += 1;
        }
    }
    println!("{:?}", match_count);

    Ok(match_count >= 1)
}

#[cfg(test)]
mod tests {
    use io::Read;

    use super::*;

    fn load_image(image_path: &str) -> Vec<u8> {
        let mut file = File::open(image_path).expect("No se pudo abrir imagen");
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)
            .expect("No se pudo leer imagen");
        buffer
    }

    #[test]
    fn test_es_incidente_false() {
        let image_data = load_image("rio.jpeg");
        let result = match es_incidente(&image_data) {
            Ok(r) => r,
            Err(e) => {
                println!("El error es: {}", e.to_string());
                assert!(false);
                return;
            },
        };
        assert!(!result, "No se esperaba incidente para rio.jpeg");
    }

    #[test]
    fn test_es_incidente_true() {
        let image_data = load_image("incendio.jpeg");
        let result = es_incidente(&image_data).expect("fallo al analizar imagen");
        assert!(result, "Se esperaba incidente para incendio.jpeg");
    }

    #[test]
    fn test_es_incidente_2_true() {
        let image_data = load_image("choque.jpeg");
        let result = es_incidente(&image_data).expect("fallo al analizar imagen");
        assert!(result, "Se esperaba incidente para choque.jpeg");
    }

    #[test]
    fn test_es_incidente_3_true() {
        let image_data = load_image("ladron.jpeg");
        let result = es_incidente(&image_data).expect("fallo al analizar imagen");
        assert!(result, "Se esperaba incidente para ladron.jpeg");
    }
}
