//! Optional: when the headset turns on, make the dock's PipeWire sink and source the default output and input
//! (`[audio] make_default`). The dock's sound card exists whenever it is plugged in, so it can't tell by itself
//! whether the headset is on; the daemon knows. Needs `pw-dump` and `wpctl` (PipeWire + WirePlumber).

use std::process::Command;

use serde_json::Value;

/// The default sink and source become the nodes of the PipeWire device with this USB product id (e.g. "2283").
/// Runs in a thread so the dock is never kept waiting.
pub fn make_default(product: &str) {
    let product = format!("0x{product}");
    std::thread::spawn(move || {
        let Ok(out) = Command::new("pw-dump").output() else {
            eprintln!("stealthbar: make_default needs pw-dump (PipeWire)");
            return;
        };
        let Ok(objects) = serde_json::from_slice::<Vec<Value>>(&out.stdout) else {
            return;
        };
        for id in nodes(&objects, &product) {
            let _ = Command::new("wpctl")
                .args(["set-default", &id.to_string()])
                .status();
        }
    });
}

/// The sink and source nodes (ids) that belong to a device with vendor 0x10f5 and the given product id.
fn nodes(objects: &[Value], product: &str) -> Vec<u64> {
    let props = |o: &Value| o["info"]["props"].clone();
    let devices: Vec<u64> = objects
        .iter()
        .filter(|o| o["type"] == "PipeWire:Interface:Device")
        .filter(|o| {
            let p = props(o);
            p["device.vendor.id"] == "0x10f5" && p["device.product.id"].as_str() == Some(product)
        })
        .filter_map(|o| o["id"].as_u64())
        .collect();
    objects
        .iter()
        .filter(|o| o["type"] == "PipeWire:Interface:Node")
        .filter(|o| {
            let p = props(o);
            p["device.id"]
                .as_u64()
                .is_some_and(|d| devices.contains(&d))
                && matches!(
                    p["media.class"].as_str(),
                    Some("Audio/Sink" | "Audio/Source")
                )
        })
        .filter_map(|o| o["id"].as_u64())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn finds_the_docks_sink_and_source() {
        let dump = json!([
            {"id": 159, "type": "PipeWire:Interface:Device",
             "info": {"props": {"device.vendor.id": "0x10f5", "device.product.id": "0x2283"}}},
            {"id": 60, "type": "PipeWire:Interface:Device",
             "info": {"props": {"device.vendor.id": "0x8086", "device.product.id": "0x2283"}}},
            {"id": 147, "type": "PipeWire:Interface:Node",
             "info": {"props": {"device.id": 159, "media.class": "Audio/Sink"}}},
            {"id": 116, "type": "PipeWire:Interface:Node",
             "info": {"props": {"device.id": 159, "media.class": "Audio/Source"}}},
            {"id": 67, "type": "PipeWire:Interface:Node",
             "info": {"props": {"device.id": 60, "media.class": "Audio/Sink"}}},
            {"id": 200, "type": "PipeWire:Interface:Node",
             "info": {"props": {"media.class": "Stream/Output/Audio"}}}
        ]);
        let objects = dump.as_array().unwrap();
        assert_eq!(nodes(objects, "0x2283"), vec![147, 116]);
        assert!(nodes(objects, "0x2286").is_empty());
    }
}
