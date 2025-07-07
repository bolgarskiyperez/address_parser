use anyhow::{Context, Result};
use regex::Regex;
use reqwest::Client;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Write;
use std::time::Duration;

#[derive(Debug, Serialize, Deserialize)]
struct AddressLocation {
    address: String,
    latitude: Option<f64>,
    longitude: Option<f64>,
    formatted_address: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct YandexGeoResponse {
    response: YandexResponse,
}

#[derive(Debug, Deserialize)]
struct YandexResponse {
    #[serde(rename = "GeoObjectCollection")]
    geo_object_collection: GeoObjectCollection,
}

#[derive(Debug, Deserialize)]
struct GeoObjectCollection {
    #[serde(rename = "featureMember")]
    feature_member: Vec<FeatureMember>,
}

#[derive(Debug, Deserialize)]
struct FeatureMember {
    #[serde(rename = "GeoObject")]
    geo_object: GeoObject,
}

#[derive(Debug, Deserialize)]
struct GeoObject {
    #[serde(rename = "Point")]
    point: Point,
    #[serde(rename = "metaDataProperty")]
    meta_data_property: MetaDataProperty,
}

#[derive(Debug, Deserialize)]
struct Point {
    pos: String,
}

#[derive(Debug, Deserialize)]
struct MetaDataProperty {
    #[serde(rename = "GeocoderMetaData")]
    geocoder_meta_data: GeocoderMetaData,
}

#[derive(Debug, Deserialize)]
struct GeocoderMetaData {
    text: String,
}

struct AddressParser {
    client: Client,
    yandex_api_key: String,
}

impl AddressParser {
    fn new(yandex_api_key: String) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .context("Failed to build HTTP client")?;
        Ok(Self {
            client,
            yandex_api_key,
        })
    }

    async fn fetch_page(&self, url: &str) -> Result<String> {
        let response = self.client
            .get(url)
            .send()
            .await
            .context("Failed to fetch page")?;

        let html = response.text().await.context("Failed to get response text")?;
        Ok(html)
    }

    fn extract_addresses(&self, html: &str) -> Result<Vec<String>> {
        let document = Html::parse_document(html);
        let selector = Selector::parse("span").unwrap();

        let mut addresses = Vec::new();

        // Паттерн для поиска адресов
        let address_patterns = vec![
            r"(?:ул\.|пр-т|пер\.|пл\.|бульвар|аллея)\s+[А-Яа-яёЁ\s-]+,?\s*(?:дом[аы]?|д\.?)?\s*[\dА-Яа-яёЁ\s,()/\-]+",
            r"(?:ул\.|пр-т|пер\.|пл\.|бульвар|аллея)\s+[А-Яа-яёЁ\s-]+,?\s*\d+[А-Яа-яёЁ\d\s,()/\-]*",
        ];

        let combined_pattern = address_patterns.join("|");
        let regex = Regex::new(&combined_pattern).unwrap();

        for element in document.select(&selector) {
            let text = element.text().collect::<Vec<_>>().join(" ");

            for cap in regex.find_iter(&text) {
                let address = cap.as_str().trim();
                if !address.is_empty() && address.len() > 10 {
                    let full_address = if address.starts_with("Калининград") {
                        address.to_string()
                    } else {
                        format!("Калининград, {}", address)
                    };
                    addresses.push(full_address);
                }
            }
        }

        // Дополнительное извлечение адресов по строкам
        let lines: Vec<&str> = html.lines().collect();
        for line in lines {
            if line.contains("ул.") || line.contains("пр-т") || line.contains("пер.") {
                for cap in regex.find_iter(line) {
                    let address = cap.as_str().trim();
                    if !address.is_empty() && address.len() > 10 {
                        let full_address = if address.starts_with("Калининград") {
                            address.to_string()
                        } else {
                            format!("Калининград, {}", address)
                        };
                        addresses.push(full_address);
                    }
                }
            }
        }

        // Удаляем дубликаты
        addresses.sort();
        addresses.dedup();

        Ok(addresses)
    }

    async fn geocode_address(&self, address: &str) -> Result<AddressLocation> {
        let url = format!(
            "https://geocode-maps.yandex.ru/1.x/?format=json&apikey={}&geocode={}",
            self.yandex_api_key,
            urlencoding::encode(address)
        );

        println!("Geocoding: {}", address);

        let response = self.client
            .get(&url)
            .send()
            .await
            .context("Failed to send geocoding request")?;

        if !response.status().is_success() {
            return Ok(AddressLocation {
                address: address.to_string(),
                latitude: None,
                longitude: None,
                formatted_address: None,
                error: Some(format!("HTTP error: {}", response.status())),
            });
        }

        let geo_response: YandexGeoResponse = response
            .json()
            .await
            .context("Failed to parse geocoding response")?;

        if let Some(feature) = geo_response.response.geo_object_collection.feature_member.first() {
            let coords: Vec<&str> = feature.geo_object.point.pos.split_whitespace().collect();
            if coords.len() == 2 {
                let longitude = match coords[0].parse::<f64>() {
                    Ok(lon) => Some(lon),
                    Err(e) => {
                        return Ok(AddressLocation {
                            address: address.to_string(),
                            latitude: None,
                            longitude: None,
                            formatted_address: None,
                            error: Some(format!("Failed to parse longitude: {}", e)),
                        });
                    }
                };
                let latitude = match coords[1].parse::<f64>() {
                    Ok(lat) => Some(lat),
                    Err(e) => {
                        return Ok(AddressLocation {
                            address: address.to_string(),
                            latitude: None,
                            longitude: None,
                            formatted_address: None,
                            error: Some(format!("Failed to parse latitude: {}", e)),
                        });
                    }
                };

                return Ok(AddressLocation {
                    address: address.to_string(),
                    latitude,
                    longitude,
                    formatted_address: Some(feature.geo_object.meta_data_property.geocoder_meta_data.text.clone()),
                    error: None,
                });
            }
        }

        Ok(AddressLocation {
            address: address.to_string(),
            latitude: None,
            longitude: None,
            formatted_address: None,
            error: Some("No coordinates found".to_string()),
        })
    }

    async fn process_addresses(&self, addresses: Vec<String>) -> Result<Vec<AddressLocation>> {
        let mut results = Vec::new();

        for address in addresses {
            match self.geocode_address(&address).await {
                Ok(location) => results.push(location),
                Err(e) => {
                    println!("Error geocoding {}: {}", address, e);
                    results.push(AddressLocation {
                        address: address.clone(),
                        latitude: None,
                        longitude: None,
                        formatted_address: None,
                        error: Some(e.to_string()),
                    });
                }
            }

            // Задержка между запросами для соблюдения лимитов API
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        Ok(results)
    }

    fn save_to_json(&self, results: &[AddressLocation], filename: &str) -> Result<()> {
        let json = serde_json::to_string_pretty(results)
            .context("Failed to serialize results to JSON")?;

        let mut file = File::create(filename)
            .context("Failed to create output file")?;

        file.write_all(json.as_bytes())
            .context("Failed to write to file")?;

        println!("Results saved to {}", filename);
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok(); // Добавьте эту строку

    // Замените на ваш API ключ Яндекс.Карт
    let yandex_api_key = std::env::var("YANDEX_API_KEY")
        .context("YANDEX_API_KEY environment variable not set. Please set it before running the program.")?;

    let parser = AddressParser::new(yandex_api_key)?;

    // URL страницы для парсинга
    let url = "https://rspoko.ru/cbor-othodov-plastika-ot-naseleniya";

    println!("Fetching page content...");
    let html = parser.fetch_page(url).await?;

    println!("Extracting addresses...");
    let addresses = parser.extract_addresses(&html)?;

    println!("Found {} addresses", addresses.len());
    for (i, address) in addresses.iter().enumerate() {
        println!("{}. {}", i + 1, address);
    }

    if addresses.is_empty() {
        println!("No addresses found. Check the parsing logic.");
        return Ok(());
    }

    println!("\nStarting geocoding...");
    let results = parser.process_addresses(addresses).await?;

    println!("\nProcessing complete!");
    println!("Successfully geocoded: {}", results.iter().filter(|r| r.latitude.is_some()).count());
    println!("Failed to geocode: {}", results.iter().filter(|r| r.latitude.is_none()).count());

    parser.save_to_json(&results, "addresses_with_coordinates.json")?;

    Ok(())
}