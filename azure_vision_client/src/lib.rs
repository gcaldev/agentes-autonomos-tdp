use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::error::Error;

#[derive(Serialize, Deserialize, Debug)]
struct Category {
    name: String,
    score: f64,
}

#[derive(Serialize, Deserialize, Debug)]
struct Tag {
    name: String,
    confidence: f64,
}

#[derive(Serialize, Deserialize, Debug)]
struct Metadata {
    height: u32,
    width: u32,
    format: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct AzureResponse {
    #[serde(rename = "tags")]
    tags: Vec<Tag>,
    #[serde(rename = "requestId")]
    request_id: String,
    #[serde(rename = "metadata")]
    metadata: Metadata,
    #[serde(rename = "modelVersion")]
    model_version: String,
}

/// Se conecta con Azure_IA y le pasa una imagen. Y retorna los tags relacionados a la imagen.
pub fn analyze_image(image_data: &[u8]) -> Result<Vec<String>, Box<dyn Error>> {
    let url = "https://brfiubaautonomos.cognitiveservices.azure.com/vision/v3.2/analyze?visualFeatures=tags&language=es";
    let subscription_key = "10ed43da4d0047928af6415a3645023c";

    let client = Client::new();

    // Realizar la solicitud POST
    let response = client
        .post(url)
        .header("Ocp-Apim-Subscription-Key", subscription_key)
        .header("Content-Type", "application/octet-stream")
        .body(image_data.to_vec())
        .send()?;

    if response.status().is_success() {
        let mut json_response: AzureResponse = response.json()?;

        let tags: Vec<String> = json_response
            .tags
            .iter_mut()
            .flat_map(|tag| tag.name.split_whitespace())
            .map(|s| s.to_string())
            .collect();

        println!("{:?}", tags);
        Ok(tags)
    } else {
        Err(format!("Error: {}", response.status()).into())
    }
}
