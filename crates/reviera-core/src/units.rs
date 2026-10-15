// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Data types and units (spec Ch 2 §2.9.4, §2.9.5, DM-PARAM-004, DM-UNIT-001).
//!
//! Values are stored in SI. Units convert only at input (parsing) and output (formatting):
//! `si = value × scale + offset`, with exact factors (1 in = 0.0254 m).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Physical dimension shared by a data type and its units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dim {
    None,
    Length,
    Area,
    Volume,
    Angle,
    Slope,
    Ratio,
    Mass,
    MassDensity,
    Time,
    Speed,
    Force,
    LinearForce,
    AreaForce,
    Moment,
    Pressure,
    Temperature,
    Flow,
    Power,
    PowerDensity,
    Energy,
    ThermalConductivity,
    HeatTransfer,
    ThermalResistance,
    SpecificHeat,
    Current,
    Voltage,
    ApparentPower,
    Illuminance,
    LuminousFlux,
    Frequency,
    Currency,
}

/// A data type (§2.9.4).
#[derive(Debug, Clone, Copy)]
pub struct DataType {
    pub key: &'static str,
    pub dim: Dim,
    /// IFC value type written for this data type.
    pub ifc_type: &'static str,
}

macro_rules! types {
    ($($key:literal => $dim:ident, $ifc:literal;)*) => {
        /// The data-type catalogue (common and the main discipline types).
        pub const DATA_TYPES: &[DataType] = &[$(DataType { key: $key, dim: Dim::$dim, ifc_type: $ifc },)*];
    };
}

types! {
    "text" => None, "IfcLabel";
    "multiline_text" => None, "IfcText";
    "integer" => None, "IfcInteger";
    "number" => None, "IfcReal";
    "yes_no" => None, "IfcBoolean";
    "url" => None, "IfcURIReference";
    "date" => None, "IfcDate";
    "datetime" => None, "IfcDateTime";
    "length" => Length, "IfcLengthMeasure";
    "distance" => Length, "IfcLengthMeasure";
    "area" => Area, "IfcAreaMeasure";
    "volume" => Volume, "IfcVolumeMeasure";
    "angle" => Angle, "IfcPlaneAngleMeasure";
    "rotation_angle" => Angle, "IfcPlaneAngleMeasure";
    "slope" => Slope, "IfcRatioMeasure";
    "ratio" => Ratio, "IfcRatioMeasure";
    "percentage" => Ratio, "IfcPositiveRatioMeasure";
    "currency" => Currency, "IfcMonetaryMeasure";
    "mass" => Mass, "IfcMassMeasure";
    "mass_density" => MassDensity, "IfcMassDensityMeasure";
    "time" => Time, "IfcTimeMeasure";
    "speed" => Speed, "IfcLinearVelocityMeasure";
    "velocity" => Speed, "IfcLinearVelocityMeasure";
    "force" => Force, "IfcForceMeasure";
    "linear_force" => LinearForce, "IfcLinearForceMeasure";
    "area_force" => AreaForce, "IfcPlanarForceMeasure";
    "moment" => Moment, "IfcTorqueMeasure";
    "stress" => Pressure, "IfcPressureMeasure";
    "pressure" => Pressure, "IfcPressureMeasure";
    "temperature" => Temperature, "IfcThermodynamicTemperatureMeasure";
    "airflow" => Flow, "IfcVolumetricFlowRateMeasure";
    "flow" => Flow, "IfcVolumetricFlowRateMeasure";
    "power" => Power, "IfcPowerMeasure";
    "power_density" => PowerDensity, "IfcHeatFluxDensityMeasure";
    "energy" => Energy, "IfcEnergyMeasure";
    "thermal_conductivity" => ThermalConductivity, "IfcThermalConductivityMeasure";
    "heat_transfer_coefficient" => HeatTransfer, "IfcThermalTransmittanceMeasure";
    "thermal_resistance" => ThermalResistance, "IfcThermalResistanceMeasure";
    "specific_heat" => SpecificHeat, "IfcSpecificHeatCapacityMeasure";
    "current" => Current, "IfcElectricCurrentMeasure";
    "electrical_potential" => Voltage, "IfcElectricVoltageMeasure";
    "apparent_power" => ApparentPower, "IfcPowerMeasure";
    "illuminance" => Illuminance, "IfcIlluminanceMeasure";
    "luminous_flux" => LuminousFlux, "IfcLuminousFluxMeasure";
    "frequency" => Frequency, "IfcFrequencyMeasure";
}

/// Class of the project-settings element (RXD document `project/settings`).
pub const SETTINGS_CLASS: &str = "rxd:project/settings";
/// Parameter of the settings element holding [`UnitSettings`].
pub const SETTINGS_KEY: &str = "units";

/// The project's unit settings (defaults when none are stored).
pub fn settings_of(store: &crate::store::Store) -> UnitSettings {
    store
        .get(crate::id::ElementId::PROJECT_SETTINGS)
        .and_then(|e| e.param(SETTINGS_KEY))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Replaces unit strings in command arguments with SI numbers: every top-level property whose
/// schema has `x-reviera-data-type` accepts `"1200 mm"` as well as a number (DM-CMD-001).
pub fn convert_arguments(
    schema: &serde_json::Value,
    args: &mut serde_json::Value,
    settings: &UnitSettings,
) -> Result<()> {
    convert_at(schema, schema, args, settings, "")
}

/// Converts the unit strings in `value` against `schema`, following `$ref`s into `$defs`,
/// nested properties, array items and `oneOf`/`anyOf`/`allOf` variants.
fn convert_at(
    root: &serde_json::Value,
    schema: &serde_json::Value,
    value: &mut serde_json::Value,
    settings: &UnitSettings,
    path: &str,
) -> Result<()> {
    use serde_json::Value;
    let schema = match schema.get("$ref").and_then(Value::as_str) {
        Some(r) => r
            .strip_prefix("#/")
            .map(|p| p.split('/').fold(root, |s, part| &s[part]))
            .unwrap_or(&Value::Null),
        None => schema,
    };
    if let (Some(dt), Value::String(text)) = (
        schema.get("x-reviera-data-type").and_then(Value::as_str),
        &*value,
    ) {
        let si = parse(text, dt, settings).map_err(|e| {
            Error::new(
                "args.invalid",
                format!("`{}`: {}", path.trim_start_matches('.'), e.message),
            )
        })?;
        *value = serde_json::json!(si);
        return Ok(());
    }
    for key in ["oneOf", "anyOf", "allOf"] {
        for variant in schema
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            convert_at(root, variant, value, settings, path)?;
        }
    }
    match value {
        Value::Object(map) => {
            if let Some(props) = schema.get("properties").and_then(Value::as_object) {
                for (name, prop) in props {
                    if let Some(v) = map.get_mut(name) {
                        convert_at(root, prop, v, settings, &format!("{path}.{name}"))?;
                    }
                }
            }
        }
        Value::Array(items) => {
            if let Some(item) = schema.get("items") {
                for (i, v) in items.iter_mut().enumerate() {
                    convert_at(root, item, v, settings, &format!("{path}[{i}]"))?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn data_type(key: &str) -> Option<&'static DataType> {
    DATA_TYPES.iter().find(|t| t.key == key)
}

/// The data type for an IFC measure type, for example `IfcThermalTransmittanceMeasure` →
/// `heat_transfer_coefficient` (§2.9.4 acceptance). The first catalogue entry wins.
pub fn data_type_for_ifc(ifc_type: &str) -> Option<&'static DataType> {
    let ifc = match ifc_type {
        "IfcPositiveLengthMeasure" | "IfcNonNegativeLengthMeasure" => "IfcLengthMeasure",
        "IfcPositivePlaneAngleMeasure" => "IfcPlaneAngleMeasure",
        "IfcQuantityLength" => "IfcLengthMeasure",
        "IfcQuantityArea" => "IfcAreaMeasure",
        "IfcQuantityVolume" => "IfcVolumeMeasure",
        "IfcQuantityWeight" => "IfcMassMeasure",
        "IfcQuantityTime" => "IfcTimeMeasure",
        other => other,
    };
    DATA_TYPES
        .iter()
        .find(|t| t.ifc_type == ifc && t.dim != Dim::None)
}

/// A unit: `si = value × scale + offset`.
#[derive(Debug, Clone, Copy)]
pub struct Unit {
    pub key: &'static str,
    pub dim: Dim,
    /// The first symbol is the default display symbol.
    pub symbols: &'static [&'static str],
    pub scale: f64,
    pub offset: f64,
}

const IN: f64 = 0.0254;
const FT: f64 = 0.3048;
const LB: f64 = 0.453_592_37;
const LBF: f64 = 4.448_221_615_260_5;
const GAL_US: f64 = 0.003_785_411_784;
const GAL_UK: f64 = 0.004_546_09;
const BTU_H: f64 = 0.293_071_070_172_222_2;

macro_rules! units {
    ($($key:literal, $dim:ident, [$($sym:literal),*], $scale:expr $(, $offset:expr)?;)*) => {
        /// The unit catalogue (§2.9.5 minimum set).
        pub const UNITS: &[Unit] = &[$(Unit {
            key: $key, dim: Dim::$dim, symbols: &[$($sym),*], scale: $scale, offset: 0.0 $(+ $offset)?
        },)*];
    };
}

units! {
    "millimetre", Length, ["mm"], 0.001;
    "centimetre", Length, ["cm"], 0.01;
    "decimetre", Length, ["dm"], 0.1;
    "metre", Length, ["m"], 1.0;
    "kilometre", Length, ["km"], 1000.0;
    "inch", Length, ["\"", "in"], IN;
    "foot", Length, ["'", "ft"], FT;
    "feet_inches", Length, ["'\""], IN;
    "yard", Length, ["yd"], 0.9144;
    "mile", Length, ["mi"], 1609.344;
    "square_millimetre", Area, ["mm²", "mm2"], 1e-6;
    "square_centimetre", Area, ["cm²", "cm2"], 1e-4;
    "square_metre", Area, ["m²", "m2"], 1.0;
    "hectare", Area, ["ha"], 1e4;
    "square_kilometre", Area, ["km²", "km2"], 1e6;
    "square_inch", Area, ["in²", "in2"], IN * IN;
    "square_foot", Area, ["ft²", "ft2", "SF"], FT * FT;
    "acre", Area, ["ac"], 4_046.856_422_4;
    "cubic_millimetre", Volume, ["mm³", "mm3"], 1e-9;
    "cubic_centimetre", Volume, ["cm³", "cm3"], 1e-6;
    "litre", Volume, ["L", "l"], 1e-3;
    "cubic_metre", Volume, ["m³", "m3"], 1.0;
    "cubic_inch", Volume, ["in³", "in3"], IN * IN * IN;
    "cubic_foot", Volume, ["ft³", "ft3", "CF"], FT * FT * FT;
    "cubic_yard", Volume, ["yd³", "yd3", "CY"], 0.9144 * 0.9144 * 0.9144;
    "us_gallon", Volume, ["gal", "US gal"], GAL_US;
    "uk_gallon", Volume, ["UK gal"], GAL_UK;
    "degree", Angle, ["°", "deg"], std::f64::consts::PI / 180.0;
    "radian", Angle, ["rad"], 1.0;
    "gradian", Angle, ["grad", "gon"], std::f64::consts::PI / 200.0;
    "percent_slope", Slope, ["%"], 0.01;
    "rise_per_run", Slope, [":"], 1.0;
    "rise_per_12_inches", Slope, ["\"/12\""], 1.0 / 12.0;
    "slope_ratio", Slope, ["/1"], 1.0;
    "ratio", Ratio, [""], 1.0;
    "percent", Ratio, ["%"], 0.01;
    "kilogram", Mass, ["kg"], 1.0;
    "gram", Mass, ["g"], 1e-3;
    "tonne", Mass, ["t"], 1000.0;
    "pound", Mass, ["lb"], LB;
    "kip_mass", Mass, ["kip"], 1000.0 * LB;
    "kilogram_per_cubic_metre", MassDensity, ["kg/m³", "kg/m3"], 1.0;
    "second", Time, ["s"], 1.0;
    "minute", Time, ["min"], 60.0;
    "hour", Time, ["h"], 3600.0;
    "metre_per_second", Speed, ["m/s"], 1.0;
    "kilometre_per_hour", Speed, ["km/h"], 1.0 / 3.6;
    "foot_per_minute", Speed, ["fpm", "ft/min"], FT / 60.0;
    "mile_per_hour", Speed, ["mph"], 1609.344 / 3600.0;
    "newton", Force, ["N"], 1.0;
    "kilonewton", Force, ["kN"], 1000.0;
    "pound_force", Force, ["lbf"], LBF;
    "kip_force", Force, ["kip", "kipf"], 1000.0 * LBF;
    "newton_per_metre", LinearForce, ["N/m"], 1.0;
    "kilonewton_per_metre", LinearForce, ["kN/m"], 1000.0;
    "newton_per_square_metre", AreaForce, ["N/m²", "N/m2"], 1.0;
    "kilonewton_per_square_metre", AreaForce, ["kN/m²", "kN/m2"], 1000.0;
    "newton_metre", Moment, ["N·m", "Nm"], 1.0;
    "kilonewton_metre", Moment, ["kN·m", "kNm"], 1000.0;
    "pascal", Pressure, ["Pa"], 1.0;
    "kilopascal", Pressure, ["kPa"], 1000.0;
    "megapascal", Pressure, ["MPa"], 1e6;
    "bar", Pressure, ["bar"], 1e5;
    "psi", Pressure, ["psi"], LBF / (IN * IN);
    "inch_of_water", Pressure, ["in H₂O", "inH2O", "in wg"], 249.082;
    "inch_of_mercury", Pressure, ["in Hg", "inHg"], 3386.389;
    "kelvin", Temperature, ["K"], 1.0;
    "degree_celsius", Temperature, ["°C", "C"], 1.0, 273.15;
    "degree_fahrenheit", Temperature, ["°F", "F"], 5.0 / 9.0, 459.67 * 5.0 / 9.0;
    "degree_rankine", Temperature, ["°R"], 5.0 / 9.0;
    "cubic_metre_per_second", Flow, ["m³/s", "m3/s"], 1.0;
    "litre_per_second", Flow, ["L/s", "l/s"], 1e-3;
    "cubic_metre_per_hour", Flow, ["m³/h", "m3/h"], 1.0 / 3600.0;
    "cubic_foot_per_minute", Flow, ["CFM", "cfm"], FT * FT * FT / 60.0;
    "us_gallon_per_minute", Flow, ["GPM", "gpm"], GAL_US / 60.0;
    "watt", Power, ["W"], 1.0;
    "kilowatt", Power, ["kW"], 1000.0;
    "btu_per_hour", Power, ["BTU/h", "Btu/h"], BTU_H;
    "ton_of_refrigeration", Power, ["TR", "ton"], 12000.0 * BTU_H;
    "watt_per_square_metre", PowerDensity, ["W/m²", "W/m2"], 1.0;
    "joule", Energy, ["J"], 1.0;
    "kilojoule", Energy, ["kJ"], 1000.0;
    "kilowatt_hour", Energy, ["kWh"], 3.6e6;
    "watt_per_metre_kelvin", ThermalConductivity, ["W/(m·K)", "W/mK"], 1.0;
    "watt_per_square_metre_kelvin", HeatTransfer, ["W/(m²·K)", "W/m2K"], 1.0;
    "square_metre_kelvin_per_watt", ThermalResistance, ["(m²·K)/W", "m2K/W"], 1.0;
    "joule_per_kilogram_kelvin", SpecificHeat, ["J/(kg·K)", "J/kgK"], 1.0;
    "ampere", Current, ["A"], 1.0;
    "volt", Voltage, ["V"], 1.0;
    "volt_ampere", ApparentPower, ["VA"], 1.0;
    "kilovolt_ampere", ApparentPower, ["kVA"], 1000.0;
    "lux", Illuminance, ["lx"], 1.0;
    "footcandle", Illuminance, ["fc"], 1.0 / (FT * FT);
    "lumen", LuminousFlux, ["lm"], 1.0;
    "hertz", Frequency, ["Hz"], 1.0;
    "currency", Currency, ["¤"], 1.0;
}

pub fn unit(key: &str) -> Option<&'static Unit> {
    UNITS.iter().find(|u| u.key == key)
}

/// How one data type is displayed (§2.9.5 Format dialog).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Format {
    /// Unit key, for example `millimetre` or `feet_inches`.
    pub unit: String,
    /// Decimal places (0–12), when no increment or fraction is set.
    pub decimals: u8,
    /// Round to a multiple of this value, in SI (for example 0.005 for 5 mm).
    pub increment: Option<f64>,
    /// Round inches to 1/n (2–256, powers of two), for `feet_inches` and `inch`.
    pub fraction: Option<u16>,
    /// Show the unit symbol.
    pub symbol: bool,
    pub suppress_trailing_zeros: bool,
    pub suppress_zero_feet: bool,
    pub suppress_zero_inches: bool,
    pub plus_sign: bool,
    pub digit_grouping: bool,
    pub suppress_spaces: bool,
}

impl Default for Format {
    fn default() -> Self {
        Self {
            unit: "metre".into(),
            decimals: 2,
            increment: None,
            fraction: None,
            symbol: true,
            suppress_trailing_zeros: false,
            suppress_zero_feet: false,
            suppress_zero_inches: false,
            plus_sign: false,
            digit_grouping: false,
            suppress_spaces: false,
        }
    }
}

impl Format {
    fn new(unit: &str, decimals: u8, symbol: bool) -> Self {
        Self {
            unit: unit.into(),
            decimals,
            symbol,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    MetricMm,
    MetricM,
    ImperialFtIn,
    ImperialDecimalFt,
}

/// Project display units (RXD document `project/settings`, key `units`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct UnitSettings {
    pub formats: BTreeMap<String, Format>,
    pub decimal_symbol: String,
    /// Digit-grouping symbol: `,`, `.`, ` ` or `'`.
    pub grouping_symbol: String,
    /// Group digits in threes, or Indian style (`12,34,567`).
    pub indian_grouping: bool,
}

impl Default for UnitSettings {
    fn default() -> Self {
        Self::preset(Preset::MetricMm)
    }
}

impl UnitSettings {
    pub fn preset(preset: Preset) -> Self {
        use Preset::*;
        let imperial = matches!(preset, ImperialFtIn | ImperialDecimalFt);
        let mut f = BTreeMap::new();
        let length = match preset {
            MetricMm => Format::new("millimetre", 0, false),
            MetricM => Format::new("metre", 3, true),
            ImperialFtIn => Format {
                fraction: Some(256),
                ..Format::new("feet_inches", 0, true)
            },
            ImperialDecimalFt => Format::new("foot", 2, true),
        };
        f.insert("length".into(), length.clone());
        f.insert("distance".into(), length);
        let pick = |m: (&str, u8), i: (&str, u8)| {
            if imperial {
                Format::new(i.0, i.1, true)
            } else {
                Format::new(m.0, m.1, true)
            }
        };
        f.insert("area".into(), pick(("square_metre", 2), ("square_foot", 2)));
        f.insert("volume".into(), pick(("cubic_metre", 2), ("cubic_foot", 2)));
        f.insert("angle".into(), Format::new("degree", 2, true));
        f.insert("rotation_angle".into(), Format::new("degree", 2, true));
        f.insert("slope".into(), Format::new("percent_slope", 2, true));
        f.insert("ratio".into(), Format::new("ratio", 2, false));
        f.insert("percentage".into(), Format::new("percent", 1, true));
        f.insert("mass".into(), pick(("kilogram", 2), ("pound", 2)));
        f.insert(
            "temperature".into(),
            pick(("degree_celsius", 1), ("degree_fahrenheit", 1)),
        );
        f.insert("pressure".into(), pick(("pascal", 1), ("psi", 2)));
        f.insert(
            "airflow".into(),
            pick(("litre_per_second", 1), ("cubic_foot_per_minute", 0)),
        );
        f.insert("power".into(), pick(("watt", 0), ("btu_per_hour", 0)));
        f.insert(
            "speed".into(),
            pick(("metre_per_second", 2), ("foot_per_minute", 0)),
        );
        f.insert("illuminance".into(), pick(("lux", 0), ("footcandle", 1)));
        Self {
            formats: f,
            decimal_symbol: ".".into(),
            grouping_symbol: ",".into(),
            indian_grouping: false,
        }
    }

    /// The display format of a data type: the project setting, else SI with 2 decimals.
    pub fn format_for(&self, data_type: &DataType) -> Format {
        if let Some(f) = self.formats.get(data_type.key) {
            return f.clone();
        }
        let si = UNITS
            .iter()
            .find(|u| u.dim == data_type.dim && u.scale == 1.0 && u.offset == 0.0);
        Format::new(si.map_or("ratio", |u| u.key), 2, true)
    }

    pub fn validate(&self) -> Result<()> {
        if self.decimal_symbol == self.grouping_symbol {
            return Err(Error::new(
                "units.same_separators",
                "the decimal symbol and the digit-grouping symbol must differ",
            ));
        }
        if ![".", ","].contains(&self.decimal_symbol.as_str()) {
            return Err(Error::new(
                "units.invalid_format",
                "the decimal symbol is `.` or `,`",
            ));
        }
        for (dt, f) in &self.formats {
            let t = data_type(dt).ok_or_else(|| {
                Error::new("units.unknown_data_type", format!("no data type `{dt}`"))
            })?;
            validate_format(t, f)?;
        }
        Ok(())
    }
}

pub fn validate_format(t: &DataType, f: &Format) -> Result<()> {
    let u = unit(&f.unit)
        .ok_or_else(|| Error::new("units.unknown_unit", format!("no unit `{}`", f.unit)))?;
    if u.dim != t.dim {
        return Err(Error::new(
            "units.wrong_dimension",
            format!("`{}` cannot display `{}` values", f.unit, t.key),
        ));
    }
    if f.decimals > 12 {
        return Err(Error::new(
            "units.invalid_format",
            "decimal places range from 0 to 12",
        ));
    }
    if let Some(i) = f.increment
        && !(i > 0.0 && i.is_finite())
    {
        return Err(Error::new(
            "units.invalid_format",
            "a rounding increment must be positive",
        ));
    }
    if let Some(n) = f.fraction
        && (!(2..=256).contains(&n)
            || !n.is_power_of_two()
            || !matches!(u.key, "feet_inches" | "inch"))
    {
        return Err(Error::new(
            "units.invalid_format",
            "fractional rounding is 1/2 to 1/256, for feet-and-inches and inches only",
        ));
    }
    Ok(())
}

fn dt(key: &str) -> Result<&'static DataType> {
    data_type(key)
        .ok_or_else(|| Error::new("units.unknown_data_type", format!("no data type `{key}`")))
}

/// Formats an SI value for display (`units.format`).
pub fn format(
    value: f64,
    data_type: &str,
    settings: &UnitSettings,
    format: Option<&Format>,
) -> Result<String> {
    let t = dt(data_type)?;
    let f = format.cloned().unwrap_or_else(|| settings.format_for(t));
    validate_format(t, &f)?;
    let u = unit(&f.unit).expect("validated");
    let sp = if f.suppress_spaces { "" } else { " " };
    let sign = if value < 0.0 {
        "-"
    } else if f.plus_sign && value > 0.0 {
        "+"
    } else {
        ""
    };

    if u.key == "feet_inches" {
        let inches = round_inches(value.abs() / IN, &f);
        let mut feet = (inches / 12.0).floor();
        let mut rest = inches - feet * 12.0;
        if (rest - 12.0).abs() < 1e-9 {
            feet += 1.0;
            rest = 0.0;
        }
        let inch_text = inch_number(rest, &f, settings);
        let show_feet = !(f.suppress_zero_feet && feet == 0.0);
        let show_inches = !(f.suppress_zero_inches && rest.abs() < 1e-9 && show_feet);
        let mut out = String::from(sign);
        if show_feet {
            out.push_str(&format!("{}'", group(&format!("{feet:.0}"), settings, &f)));
        }
        if show_feet && show_inches {
            out.push_str(&format!("{sp}-{sp}"));
        }
        if show_inches {
            out.push_str(&format!("{inch_text}\""));
        }
        return Ok(out);
    }

    let shown = (value - u.offset) / u.scale;
    let sign = if shown < 0.0 {
        "-"
    } else if f.plus_sign && shown > 0.0 {
        "+"
    } else {
        ""
    };
    let shown = shown.abs();
    let text = if u.key == "inch" && f.fraction.is_some() {
        inch_number(round_inches(shown, &f), &f, settings)
    } else {
        let rounded = match f.increment {
            Some(i) => {
                let step = i / u.scale;
                (shown / step).round() * step
            }
            None => shown,
        };
        number(rounded, f.decimals, &f, settings)
    };
    let symbol = if f.symbol {
        u.symbols.first().copied().unwrap_or("")
    } else {
        ""
    };
    let joiner = if symbol.is_empty() || matches!(symbol, "°" | "%" | "\"" | "'") {
        ""
    } else {
        sp
    };
    Ok(format!("{sign}{text}{joiner}{symbol}"))
}

fn round_inches(inches: f64, f: &Format) -> f64 {
    match (f.fraction, f.increment) {
        (Some(n), _) => (inches * n as f64).round() / n as f64,
        (None, Some(i)) => (inches / (i / IN)).round() * (i / IN),
        (None, None) => {
            let p = 10f64.powi(f.decimals as i32);
            (inches * p).round() / p
        }
    }
}

/// Inches as `3 3/8` (fraction format) or decimals.
fn inch_number(inches: f64, f: &Format, settings: &UnitSettings) -> String {
    let Some(n) = f.fraction else {
        return number(inches, f.decimals, f, settings);
    };
    let whole = inches.floor();
    let mut num = ((inches - whole) * n as f64).round() as u32;
    let mut den = n as u32;
    if num == 0 {
        return format!("{whole:.0}");
    }
    while num.is_multiple_of(2) && den > 1 {
        num /= 2;
        den /= 2;
    }
    if whole == 0.0 {
        format!("{num}/{den}")
    } else {
        format!("{whole:.0} {num}/{den}")
    }
}

fn number(value: f64, decimals: u8, f: &Format, settings: &UnitSettings) -> String {
    let mut s = format!("{:.*}", decimals as usize, value.abs());
    if f.suppress_trailing_zeros && s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_owned();
    }
    let (int, frac) = s
        .split_once('.')
        .map_or((s.as_str(), None), |(i, d)| (i, Some(d)));
    let int = group(int, settings, f);
    match frac {
        Some(d) => format!("{int}{}{d}", settings.decimal_symbol),
        None => int,
    }
}

fn group(int: &str, settings: &UnitSettings, f: &Format) -> String {
    if !f.digit_grouping || int.len() <= 3 {
        return int.to_owned();
    }
    let digits: Vec<char> = int.chars().collect();
    let mut groups = Vec::new();
    let mut end = digits.len();
    let mut size = 3;
    while end > 0 {
        let start = end.saturating_sub(size);
        groups.push(digits[start..end].iter().collect::<String>());
        end = start;
        if settings.indian_grouping {
            size = 2;
        }
    }
    groups.reverse();
    groups.join(&settings.grouping_symbol)
}

/// Parses user input to SI (`units.parse`). Accepts any unit of the right dimension (`3' 6"`,
/// `5'-6 1/2"`, `1067`, `1.067 m`, `42 in`, `20 °C`); a bare number is in the display unit.
/// Arithmetic is accepted too (ARC-DIM-002): `1200+600`, `(2*900)/2`, `3 m - 450`, with `+ - * /`
/// (`×` too) and parentheses; terms are values as above, worked in the display unit.
pub fn parse(text: &str, data_type: &str, settings: &UnitSettings) -> Result<f64> {
    // Arithmetic when there is an operator other than a fraction's `/` or the `-` of `5'-6"`.
    let arithmetic = tokenize(text).iter().enumerate().any(|(i, t)| {
        matches!(t, Token::Op('+' | '*' | '(' | ')')) || (*t == Token::Op('-') && i > 0)
    });
    let plain = if arithmetic {
        Err(Error::new(
            "units.parse_failed",
            format!("`{text}` is arithmetic"),
        ))
    } else {
        parse_value(text, data_type, settings)
    };
    match plain {
        Ok(v) => Ok(v),
        Err(_) => {
            let e = parse_value(text, data_type, settings)
                .err()
                .unwrap_or_else(|| {
                    Error::new("units.parse_failed", format!("`{text}` is not a value"))
                });
            let has_operator = text.contains(['+', '*', '×', '(', ')', '-', '/']);
            if !has_operator {
                return Err(e);
            }
            let t = dt(data_type)?;
            // The unit a bare number is in, which the arithmetic is worked in.
            let display = unit(&settings.format_for(t).unit).expect("format units exist");
            let bare = if display.key == "feet_inches" {
                unit("inch").expect("inch")
            } else {
                display
            };
            let tokens = tokenize(text);
            let mut pos = 0;
            let leaf = |s: &str| {
                parse_value(s, data_type, settings).map(|v| (v - bare.offset) / bare.scale)
            };
            let v = expr(&tokens, &mut pos, &leaf).ok_or_else(|| e.clone())?;
            if pos != tokens.len() || !v.is_finite() {
                return Err(e);
            }
            Ok(v * bare.scale + bare.offset)
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Op(char),
    Leaf(String),
}

/// Splits arithmetic into operators and value terms. A `-` right after a foot mark belongs to the
/// value (`5'-6"`).
fn tokenize(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.trim().chars() {
        let after_feet = cur.trim_end().ends_with('\'');
        let op = match c {
            '+' | '*' | '/' | '(' | ')' => Some(c),
            '×' => Some('*'),
            '-' if !after_feet => Some('-'),
            _ => None,
        };
        match op {
            Some(o) => {
                if !cur.trim().is_empty() {
                    out.push(Token::Leaf(cur.trim().to_owned()));
                }
                cur.clear();
                out.push(Token::Op(o));
            }
            None => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(Token::Leaf(cur.trim().to_owned()));
    }
    out
}

fn expr(t: &[Token], pos: &mut usize, leaf: &dyn Fn(&str) -> Result<f64>) -> Option<f64> {
    let mut v = term(t, pos, leaf)?;
    while let Some(Token::Op(o @ ('+' | '-'))) = t.get(*pos) {
        *pos += 1;
        let r = term(t, pos, leaf)?;
        v = if *o == '+' { v + r } else { v - r };
    }
    Some(v)
}

fn term(t: &[Token], pos: &mut usize, leaf: &dyn Fn(&str) -> Result<f64>) -> Option<f64> {
    let mut v = factor(t, pos, leaf)?;
    while let Some(Token::Op(o @ ('*' | '/'))) = t.get(*pos) {
        *pos += 1;
        let r = factor(t, pos, leaf)?;
        v = if *o == '*' { v * r } else { v / r };
    }
    Some(v)
}

fn factor(t: &[Token], pos: &mut usize, leaf: &dyn Fn(&str) -> Result<f64>) -> Option<f64> {
    match t.get(*pos)? {
        Token::Op('-') => {
            *pos += 1;
            factor(t, pos, leaf).map(|v| -v)
        }
        Token::Op('+') => {
            *pos += 1;
            factor(t, pos, leaf)
        }
        Token::Op('(') => {
            *pos += 1;
            let v = expr(t, pos, leaf)?;
            matches!(t.get(*pos), Some(Token::Op(')'))).then(|| {
                *pos += 1;
                v
            })
        }
        Token::Leaf(s) => {
            *pos += 1;
            leaf(s).ok()
        }
        Token::Op(_) => None,
    }
}

/// One value (no arithmetic).
fn parse_value(text: &str, data_type: &str, settings: &UnitSettings) -> Result<f64> {
    let t = dt(data_type)?;
    let bad = || {
        Error::new(
            "units.parse_failed",
            format!("`{text}` is not a {} value", t.key),
        )
    };
    let mut s = text.trim().to_owned();
    if settings.decimal_symbol == "," {
        s = s
            .replace(settings.grouping_symbol.as_str(), "")
            .replace(',', ".");
    } else if !settings.grouping_symbol.trim().is_empty() {
        s = s.replace(settings.grouping_symbol.as_str(), "");
    }
    let (negative, body) = match s.strip_prefix('-') {
        Some(rest) => (true, rest.trim_start().to_owned()),
        None => (
            false,
            s.strip_prefix('+').unwrap_or(&s).trim_start().to_owned(),
        ),
    };
    let display = unit(&settings.format_for(t).unit).expect("format units exist");

    let mut total = 0.0;
    let mut terms = 0;
    let mut feet_seen = false;
    let mut rest = body.as_str();
    // In feet and inches, two bare numbers are feet then inches (`8 6` is 8'-6").
    if display.key == "feet_inches" {
        let words: Vec<&str> = body.split_whitespace().collect();
        if let [f, i] = words[..]
            && let (Ok(f), Ok(i)) = (f.parse::<f64>(), i.parse::<f64>())
        {
            let foot = unit("foot").expect("foot");
            let inch = unit("inch").expect("inch");
            let v = f * foot.scale + i * inch.scale;
            return Ok(if negative { -v } else { v });
        }
    }
    while !rest.trim().is_empty() {
        rest = rest.trim_start().trim_start_matches('-').trim_start();
        let (value, after) = read_number(rest).ok_or_else(bad)?;
        let after = after.trim_start();
        let (u, after) = match match_unit(after, t.dim) {
            Some((u, len)) => (Some(u), &after[len..]),
            None => (None, after),
        };
        let u = match u {
            Some(u) => u,
            // After feet, a bare number is inches.
            None if feet_seen => unit("inch").expect("inch"),
            None if display.key == "feet_inches" => unit("inch").expect("inch"),
            None => display,
        };
        if u.key == "foot" {
            feet_seen = true;
        }
        if u.offset != 0.0 && terms > 0 {
            return Err(bad());
        }
        total += value * u.scale + u.offset;
        terms += 1;
        rest = after;
    }
    if terms == 0 {
        return Err(bad());
    }
    Ok(if negative { -total } else { total })
}

/// A number, a fraction (`3/8`) or a mixed number (`3 3/8`); returns the value and the rest.
fn read_number(s: &str) -> Option<(f64, &str)> {
    let (a, rest) = read_plain(s)?;
    if let Some(r) = rest.strip_prefix('/') {
        let (b, rest) = read_plain(r)?;
        return Some((a / b, rest));
    }
    let trimmed = rest.trim_start();
    if rest.len() != trimmed.len()
        && let Some((n, r)) = read_plain(trimmed)
        && let Some(r2) = r.strip_prefix('/')
        && let Some((d, r3)) = read_plain(r2)
    {
        return Some((a + n / d, r3));
    }
    Some((a, rest))
}

fn read_plain(s: &str) -> Option<(f64, &str)> {
    let end = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == 'e' || c == 'E'))
        .unwrap_or(s.len());
    // Do not swallow a unit that starts with `e` or `E`.
    let end = s[..end]
        .find(['e', 'E'])
        .filter(|&i| !s[i + 1..].starts_with(|c: char| c.is_ascii_digit() || c == '-'))
        .unwrap_or(end);
    if end == 0 {
        return None;
    }
    s[..end].parse().ok().map(|v| (v, &s[end..]))
}

/// The longest unit symbol of dimension `dim` at the start of `s`; exact case beats a
/// case-insensitive match.
fn match_unit(s: &str, dim: Dim) -> Option<(&'static Unit, usize)> {
    let mut best: Option<(&'static Unit, usize, bool)> = None;
    for u in UNITS
        .iter()
        .filter(|u| u.dim == dim && u.key != "feet_inches")
    {
        for sym in u.symbols.iter().filter(|s| !s.is_empty()) {
            let n = sym.len();
            if s.len() < n || !s.is_char_boundary(n) {
                continue;
            }
            let exact = &s[..n] == *sym;
            if !exact && !s[..n].eq_ignore_ascii_case(sym) {
                continue;
            }
            // The symbol must end the word: `m` must not match the start of `mm`.
            if s[n..].chars().next().is_some_and(char::is_alphabetic) {
                continue;
            }
            if best.is_none_or(|(_, len, ex)| (n, exact) > (len, ex)) {
                best = Some((u, n, exact));
            }
        }
    }
    best.map(|(u, n, _)| (u, n))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s() -> UnitSettings {
        UnitSettings::default()
    }

    #[test]
    fn typed_arithmetic_and_feet_inches() {
        let close = |t: &str, v: f64| (parse(t, "length", &s()).unwrap() - v).abs() < 1e-9;
        assert!(close("1200+600", 1.8));
        assert!(close("(2*900)/2", 0.9));
        assert!(close("2 × 450", 0.9));
        assert!(close("3 m - 450", 2.55));
        assert!(close("5'-6\" + 6\"", 1.6764 + 0.1524));
        assert!(parse("abc", "length", &s()).is_err());
        assert!(parse("(1200", "length", &s()).is_err());
        let imperial = UnitSettings::preset(Preset::ImperialFtIn);
        assert!((parse("8 6", "length", &imperial).unwrap() - 2.5908).abs() < 1e-9);
    }

    #[test]
    fn feet_and_inches_parse_exactly() {
        assert!((parse("5' 6\"", "length", &s()).unwrap() - 1.6764).abs() < 1e-12);
        assert!((parse("5'-6\"", "length", &s()).unwrap() - 1.6764).abs() < 1e-12);
        assert!((parse("5' 6 1/2\"", "length", &s()).unwrap() - 66.5 * 0.0254).abs() < 1e-12);
        assert!((parse("42 in", "length", &s()).unwrap() - 1.0668).abs() < 1e-12);
        assert!((parse("1.067 m", "length", &s()).unwrap() - 1.067).abs() < 1e-12);
        assert!(
            (parse("1067", "length", &s()).unwrap() - 1.067).abs() < 1e-12,
            "bare numbers use the display unit"
        );
        assert!((parse("-3 ft", "length", &s()).unwrap() + 0.9144).abs() < 1e-12);
    }

    #[test]
    fn feet_and_fractional_inches_format() {
        let mut settings = UnitSettings::preset(Preset::ImperialFtIn);
        settings.formats.get_mut("length").unwrap().fraction = Some(16);
        let text = format(1.0, "length", &settings, None).unwrap();
        assert_eq!(text, "3' - 3 3/8\"");
        let back = parse(&text, "length", &settings).unwrap();
        assert!((back - 1.0).abs() < 0.0254 / 32.0);
    }

    #[test]
    fn metric_formats_and_separators() {
        assert_eq!(format(1.2345678, "length", &s(), None).unwrap(), "1235");
        let m = Format {
            digit_grouping: true,
            ..Format::new("millimetre", 2, true)
        };
        let mut eu = s();
        eu.decimal_symbol = ",".into();
        eu.grouping_symbol = ".".into();
        assert_eq!(
            format(123.45678, "length", &eu, Some(&m)).unwrap(),
            "123.456,78 mm"
        );
        assert!((parse("123.456,78 mm", "length", &eu).unwrap() - 123.45678).abs() < 1e-9);
        let five = Format {
            increment: Some(0.005),
            decimals: 0,
            ..Format::new("millimetre", 0, false)
        };
        assert_eq!(format(1.2371, "length", &s(), Some(&five)).unwrap(), "1235");
        let mut bad = s();
        bad.grouping_symbol = ".".into();
        assert!(bad.validate().is_err(), "same decimal and grouping symbol");
    }

    #[test]
    fn temperatures_use_offsets_and_other_dimensions_convert() {
        assert!((parse("20 °C", "temperature", &s()).unwrap() - 293.15).abs() < 1e-9);
        assert!((parse("68 °F", "temperature", &s()).unwrap() - 293.15).abs() < 1e-9);
        assert_eq!(
            format(293.15, "temperature", &s(), None).unwrap(),
            "20.0 °C"
        );
        assert!((parse("90°", "angle", &s()).unwrap() - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert!((parse("2 kN", "force", &s()).unwrap() - 2000.0).abs() < 1e-12);
        assert!(parse("2 kg", "length", &s()).is_err(), "wrong dimension");
    }

    #[test]
    fn every_preset_is_valid() {
        for p in [
            Preset::MetricMm,
            Preset::MetricM,
            Preset::ImperialFtIn,
            Preset::ImperialDecimalFt,
        ] {
            UnitSettings::preset(p).validate().unwrap();
        }
    }

    #[test]
    fn ifc_measures_map_to_data_types() {
        assert_eq!(
            data_type_for_ifc("IfcThermalTransmittanceMeasure")
                .unwrap()
                .key,
            "heat_transfer_coefficient"
        );
        assert_eq!(
            data_type_for_ifc("IfcPositiveLengthMeasure").unwrap().key,
            "length"
        );
        assert_eq!(data_type_for_ifc("IfcQuantityArea").unwrap().key, "area");
        assert!(data_type_for_ifc("IfcLabel").is_none());
        let f = Format {
            fraction: Some(16),
            ..Format::new("millimetre", 0, false)
        };
        assert!(validate_format(data_type("length").unwrap(), &f).is_err());
    }
}
