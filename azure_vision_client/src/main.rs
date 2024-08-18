use azure_vision_client::analyze_image;
use std::fs::File;
use std::io::Read;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file_path = "incendio.jpeg";
    let mut file = File::open(file_path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    match analyze_image(&buffer) {
        Ok(tags) => {
            println!("Tags: {:?}", tags);
        }
        Err(e) => println!("Error: {:?}", e),
    }

    Ok(())
}
