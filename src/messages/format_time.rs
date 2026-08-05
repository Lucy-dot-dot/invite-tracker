const TIME_UNITS: &[(&str, u64); 6] = &[
    (" year", 365 * 24 * 60 * 60),
    (" month", 30 * 24 * 60 * 60),
    (" day", 24 * 60 * 60),
    ("h", 60 * 60),
    ("m", 60),
    ("s", 1),
];

pub fn format_time_diff(seconds: u64, precision_level: u8) -> String {
    let mut time_string = String::new();

    let mut precision: u8 = 0;
    let mut remaining = seconds;

    for (unit_name, unit_value) in TIME_UNITS.iter() {
        if precision >= precision_level {
            break;
        }

        let amount = remaining / unit_value;
        if amount > 0 {
            remaining %= unit_value;
            precision += 1;

            if !time_string.is_empty() {
                time_string.push_str(" ");
            }
            time_string.push_str(&format!("{}{}", amount, unit_name));

            // pluralize if unit is longer than one word
            if amount > 1 && unit_name.len() > 1 {
                time_string.push_str("s");
            }
        }
    }

    if precision == 0 {
        return "<1s".to_string();
    }

    time_string
}
