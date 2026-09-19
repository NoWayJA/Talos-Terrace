//! Host temperature sensors through Talos's file read API: `/sys/class/hwmon/hwmonN/name`
//! says what a sensor is, `temp1_input` is its reading in thousandths of a degree. Only the
//! drivers whose meaning is clear are shown; anything else is skipped rather than guessed.

/// One discovered sensor on a node: which hwmon slot, the driver's name, a plain label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sensor {
    pub index: u32,
    pub name: String,
    pub label: String,
}

/// A plain-words label for a hwmon driver name, or `None` when Terrace would only be guessing.
pub fn label_for(name: &str) -> Option<&'static str> {
    match name.trim() {
        "k10temp" | "coretemp" | "zenpower" | "cpu_thermal" | "x86_pkg_temp" => Some("CPU"),
        "nvme" => Some("NVMe"),
        "amdgpu" | "nouveau" | "i915" | "nvidia" => Some("GPU"),
        "acpitz" | "thinkpad" | "pch_cannonlake" | "pch_skylake" => Some("Board"),
        "drivetemp" => Some("Disk"),
        _ => None,
    }
}

/// `hwmon0`, `k10temp` → the sensor, numbered when several share a label.
pub fn number_duplicates(sensors: &mut [Sensor]) {
    let mut seen: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for sensor in sensors.iter() {
        *seen.entry(sensor.label.clone()).or_default() += 1;
    }
    let mut counters: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for sensor in sensors.iter_mut() {
        if seen.get(&sensor.label).copied().unwrap_or(0) > 1 {
            let counter = counters.entry(sensor.label.clone()).or_default();
            *counter += 1;
            sensor.label = format!("{} {counter}", sensor.label);
        }
    }
}

pub fn name_path(index: u32) -> String {
    format!("/sys/class/hwmon/hwmon{index}/name")
}

pub fn temperature_path(index: u32) -> String {
    format!("/sys/class/hwmon/hwmon{index}/temp1_input")
}

/// `41500` (thousandths of a degree) → 41.5; refuses readings a sensor cannot produce.
pub fn parse_millidegrees(text: &str) -> Option<f64> {
    let value: f64 = text.trim().parse::<i64>().ok()? as f64 / 1000.0;
    (-50.0..=200.0).contains(&value).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_known_drivers_and_skips_unknown_ones() {
        assert_eq!(label_for("k10temp\n"), Some("CPU"));
        assert_eq!(label_for("nvme"), Some("NVMe"));
        assert_eq!(label_for("it8688"), None);
    }

    #[test]
    fn numbers_sensors_that_share_a_label() {
        let mut sensors = vec![
            Sensor {
                index: 0,
                name: "k10temp".into(),
                label: "CPU".into(),
            },
            Sensor {
                index: 1,
                name: "nvme".into(),
                label: "NVMe".into(),
            },
            Sensor {
                index: 2,
                name: "nvme".into(),
                label: "NVMe".into(),
            },
        ];
        number_duplicates(&mut sensors);
        assert_eq!(sensors[0].label, "CPU");
        assert_eq!(sensors[1].label, "NVMe 1");
        assert_eq!(sensors[2].label, "NVMe 2");
    }

    #[test]
    fn reads_millidegrees_within_reason() {
        assert_eq!(parse_millidegrees("41500\n"), Some(41.5));
        assert_eq!(parse_millidegrees("-5000"), Some(-5.0));
        assert_eq!(parse_millidegrees("999000"), None);
        assert_eq!(parse_millidegrees("warm"), None);
    }

    #[test]
    fn paths_follow_the_hwmon_layout() {
        assert_eq!(name_path(2), "/sys/class/hwmon/hwmon2/name");
        assert_eq!(temperature_path(0), "/sys/class/hwmon/hwmon0/temp1_input");
    }
}
