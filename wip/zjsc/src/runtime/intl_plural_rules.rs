//! `Intl.PluralRules` (`IntlPluralRules.cpp`, `IntlPluralRulesPrototype.cpp`,
//! `IntlPluralRulesConstructor.cpp`): as regras de plural do CLDR de qualquer locale (`icu_plural`, sobre
//! `icu_plurals`), aplicadas aos dígitos do número formatado com as opções de dígitos da instância.
//!
//! DIVERGÊNCIAS: o expoente da notação compacta chega ao operando `c`/`e` pela diferença de casas entre o
//! valor e os dígitos escalados (arredondamento que sobe de grandeza, `999999` para `1M`, erra uma casa); o
//! locale resolvido ainda sai de `intl_locale_data.rs` (`en-US` ou `pt-BR` até a lista do CLDR entrar em
//! `supportedLocalesOf`, e o golden `plural_bun_golden.rs` acusa os locales fora dela).

use crate::host_function;
use crate::runtime::lookup::{native_entry};
use crate::runtime::default_number_format::{format_parts, NumberSettings, NumericInput, Style, UseGrouping};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::icu_plural;
use crate::runtime::intl_locale_data::Language;
use crate::runtime::intl_number_format::{numeric_input, put_digit_fields, put_rounding_fields, read_digit_options};
use crate::runtime::intl_support::{
    canonicalize_locale_list, coerce_options_to_object, construct_instance, option_enum, put, read_locale_matcher,
    resolve_locale_from, str_value, string_array, with_instance, IntlClass, IntlEnum,
};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::default_number_format::{CompactDisplay, Notation};

/// Os operandos das regras de plural do CLDR (`n`, `i`, `v`, `f`) do número já arredondado.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluralOperands {
    /// Os dígitos da parte inteira, sem zeros à esquerda (`"0"` para zero).
    pub integer: String,
    /// Os dígitos da fração visíveis, com os zeros à direita (eles contam em `v`).
    pub fraction: String,
}

impl PluralOperands {
    /// Dos dígitos da parte inteira e da fração do número formatado.
    pub fn from_decimal(integer: &str, fraction: &str) -> PluralOperands {
        let trimmed = integer.trim_start_matches('0');
        PluralOperands {
            integer: if trimmed.is_empty() { "0".to_string() } else { trimmed.to_string() },
            fraction: fraction.to_string(),
        }
    }
}

/// A categoria cardinal do idioma dos dados embutidos (`en` ou `pt`), pelas regras do CLDR em `icu_plural`.
/// Serve ao `RelativeTimeFormat` e ao `NumberFormat`, que ainda escolhem texto por `Language`.
pub fn cardinal_category(language: Language, operands: &PluralOperands) -> &'static str {
    let tag = match language {
        Language::English => "en",
        Language::Portuguese => "pt",
    };
    icu_plural::select(tag, false, &operands.integer, &operands.fraction, 0).unwrap_or("other")
}

crate::intl_enum!(PluralType { Cardinal => "cardinal", Ordinal => "ordinal" });

/// As línguas do escopo do golden `intl_more_bun.tsv` (as do CLDR que o bun resolve para o próprio locale).
const PLURAL_LANGUAGES: [&str; 39] = [
    "en", "pt", "es", "fr", "de", "it", "ja", "ko", "zh", "ar", "fa", "he", "hi", "th", "tr", "pl", "nl", "sv", "da",
    "nb", "fi", "cs", "el", "id", "vi", "uk", "ru", "ro", "hu", "bg", "hr", "sr", "cy", "ga", "lt", "lv", "mt", "sl",
    "tzm",
];

/// O estado de um `IntlPluralRules`.
struct PluralRulesState {
    locale: String,
    kind: PluralType,
    settings: NumberSettings,
}

/// `IntlPluralRules::initializePluralRules`.
fn initialize(global_object: &JSGlobalObject, locales: JSValue, options_value: JSValue) -> Result<PluralRulesState, Thrown> {
    let resolved = resolve_locale_from(global_object, locales, &[])?;
    // Os dados de plural vêm do `icu_plurals`, de qualquer locale: a primeira tag pedida (sem extensão `-u-`)
    // cuja língua está em `PLURAL_LANGUAGES` vale como locale resolvido, como no bun (`en-GB`, `de`, `ja`).
    let requested = canonicalize_locale_list(global_object, locales)?;
    let plural_locale = requested
        .first()
        .and_then(|tag| tag.split("-u-").next())
        .filter(|base| base.split('-').next().is_some_and(|language| PLURAL_LANGUAGES.contains(&language)))
        .map(str::to_string)
        .unwrap_or_else(|| resolved.locale.clone());
    let options = coerce_options_to_object(global_object, options_value)?;
    read_locale_matcher(global_object, options)?;

    let kind = option_enum::<PluralType>(global_object, options, "type", "type must be \"cardinal\" or \"ordinal\"")?
        .unwrap_or(PluralType::Cardinal);
    let notation = option_enum::<Notation>(
        global_object,
        options,
        "notation",
        "notation must be either \"standard\", \"scientific\", \"engineering\", or \"compact\"",
    )?
    .unwrap_or(Notation::Standard);
    let compact_display =
        option_enum::<CompactDisplay>(global_object, options, "compactDisplay", "compactDisplay must be either \"short\" or \"long\"")?
            .unwrap_or(CompactDisplay::Short);

    let digits = read_digit_options(global_object, options, 0, 3, notation)?;
    let mut settings = NumberSettings::defaults(resolved.language);
    settings.notation = notation;
    settings.compact_display = compact_display;
    digits.apply(&mut settings);
    Ok(PluralRulesState { locale: plural_locale, kind, settings })
}

/// Os dígitos (inteiro, fração) do número formatado com as opções da instância, sem sinal nem agrupamento.
fn visible_digits(state: &PluralRulesState, input: &NumericInput) -> (String, String) {
    let mut settings = state.settings.clone();
    settings.style = Style::Decimal;
    settings.use_grouping = UseGrouping::False;
    let parts = format_parts(&settings, input);
    let collect = |kind: &str| -> String { parts.iter().filter(|part| part.0 == kind).map(|part| part.1.as_str()).collect() };
    (collect("integer"), collect("fraction"))
}

fn is_infinite(input: &NumericInput) -> bool {
    matches!(input, NumericInput::Double(value) if !value.is_finite())
}

/// Os dígitos visíveis e o expoente compacto do número: na notação compacta o número sai escalado (`1.5M`
/// mostra `1` e `5`), e o expoente de dez que sobrou (6) é o operando `c` das regras. O expoente sai da
/// diferença entre as casas inteiras do valor e as dos dígitos escalados; fora do compacto é zero.
/// LACUNA: um arredondamento que sobe de grandeza (`999999` virando `1M`) dá o expoente uma casa abaixo.
fn visible_operands(state: &PluralRulesState, input: &NumericInput) -> (String, String, u8) {
    let (integer, fraction) = visible_digits(state, input);
    if state.settings.notation != Notation::Compact {
        return (integer, fraction, 0);
    }
    let value_digits = match input {
        NumericInput::Double(value) => {
            let magnitude = value.abs();
            if magnitude < 1.0 { 0 } else { (magnitude.log10().floor() as usize) + 1 }
        }
        NumericInput::Decimal { digits, .. } => digits.trim_start_matches('0').len(),
    };
    let exponent = value_digits.saturating_sub(integer.trim_start_matches('0').len().max(1));
    let exponent = if integer.trim_start_matches('0').is_empty() { 0 } else { exponent };
    (integer, fraction, u8::try_from(exponent).unwrap_or(0))
}

/// `IntlPluralRules::select`: a categoria do número formatado.
fn select_category(state: &PluralRulesState, input: &NumericInput) -> &'static str {
    if is_infinite(input) {
        return "other";
    }
    let (integer, fraction, exponent) = visible_operands(state, input);
    icu_plural::select(&state.locale, state.kind == PluralType::Ordinal, &integer, &fraction, exponent).unwrap_or("other")
}

/// `IntlPluralRules::selectRange`: a categoria do intervalo pelas formas visíveis das duas pontas, com as
/// regras do `type` da instância.
fn select_range_category(state: &PluralRulesState, start: &NumericInput, end: &NumericInput) -> &'static str {
    if is_infinite(start) || is_infinite(end) {
        return "other";
    }
    let (start_integer, start_fraction, start_exponent) = visible_operands(state, start);
    let (end_integer, end_fraction, end_exponent) = visible_operands(state, end);
    icu_plural::select_range(
        &state.locale,
        state.kind == PluralType::Ordinal,
        (&start_integer, &start_fraction, start_exponent),
        (&end_integer, &end_fraction, end_exponent),
    )
    .unwrap_or("other")
}

fn construct_plural_rules_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    construct_instance(global_object, call, |global_object| {
        Ok(Box::new(initialize(global_object, call.argument(0), call.argument(1))?))
    })
}

fn call_plural_rules_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    crate::runtime::collection_support::constructor_cannot_be_called_as_function("PluralRules")
}

fn select_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<PluralRulesState, _>(
        call.this_value(),
        "Intl.PluralRules.prototype.select called on value that's not a PluralRules",
        |state, _| {
            let input = numeric_input(global_object, call.argument(0))?;
            Ok(str_value(global_object.vm(), select_category(state, &input)))
        },
    )
}

fn select_range_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<PluralRulesState, _>(
        call.this_value(),
        "Intl.PluralRules.prototype.selectRange called on value that's not a PluralRules",
        |state, _| {
            if call.argument(0).is_undefined() || call.argument(1).is_undefined() {
                return Err(Thrown::type_error("start or end is undefined"));
            }
            let start = numeric_input(global_object, call.argument(0))?;
            let end = numeric_input(global_object, call.argument(1))?;
            for input in [&start, &end] {
                if matches!(input, NumericInput::Double(value) if value.is_nan()) {
                    return Err(Thrown::range_error("Passed numbers are out of range"));
                }
            }
            Ok(str_value(global_object.vm(), select_range_category(state, &start, &end)))
        },
    )
}

fn resolved_options_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<PluralRulesState, _>(
        call.this_value(),
        "Intl.PluralRules.prototype.resolvedOptions called on value that's not a PluralRules",
        |state, _| {
            let vm = global_object.vm();
            let options = crate::runtime::intl_support::new_object(global_object);
            put(global_object, &options, "locale", str_value(vm, &state.locale));
            put(global_object, &options, "type", str_value(vm, state.kind.as_str()));
            put(global_object, &options, "notation", str_value(vm, state.settings.notation.as_str()));
            if state.settings.notation == Notation::Compact {
                put(global_object, &options, "compactDisplay", str_value(vm, state.settings.compact_display.as_str()));
            }
            put_digit_fields(global_object, &options, &state.settings);
            let names: Vec<String> = icu_plural::categories(&state.locale, state.kind == PluralType::Ordinal)
                .unwrap_or_else(|| vec!["other"])
                .into_iter()
                .map(str::to_string)
                .collect();
            put(global_object, &options, "pluralCategories", string_array(global_object, &names));
            put_rounding_fields(global_object, &options, &state.settings);
            Ok(options.as_value())
        },
    )
}

host_function!(call_plural_rules, call_plural_rules_body);
host_function!(construct_plural_rules, construct_plural_rules_body);
host_function!(plural_rules_proto_select, select_body);
host_function!(plural_rules_proto_select_range, select_range_body);
host_function!(plural_rules_proto_resolved_options, resolved_options_body);

crate::intl_prototype_s_info!(
    PLURAL_RULES_PROTOTYPE_S_INFO,
    "Intl.PluralRules",
    [
        native_entry("select", plural_rules_proto_select, 1),
        native_entry("selectRange", plural_rules_proto_select_range, 2),
        native_entry("resolvedOptions", plural_rules_proto_resolved_options, 0),
    ]
);

/// `IntlPluralRulesConstructor` e `IntlPluralRulesPrototype` (`select`, `selectRange`,
/// `resolvedOptions`, da tabela estática).
pub fn install_plural_rules(global_object: &JSGlobalObject, intl: &JSObject) {
    let class = IntlClass {
        name: "PluralRules",
        length: 0,
        has_supported_locales_of: true,
        call: call_plural_rules,
        construct: construct_plural_rules,
    };
    class.install_with_table(global_object, intl, &PLURAL_RULES_PROTOTYPE_S_INFO);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cardinal(language: Language, integer: &str, fraction: &str) -> &'static str {
        cardinal_category(language, &PluralOperands::from_decimal(integer, fraction))
    }

    fn ordinal(integer: &str) -> &'static str {
        icu_plural::select("en", true, integer, "", 0).unwrap()
    }

    fn select(locale: &str, integer: &str, fraction: &str) -> &'static str {
        icu_plural::select(locale, false, integer, fraction, 0).unwrap()
    }

    #[test]
    fn english_cardinal() {
        assert_eq!(cardinal(Language::English, "1", ""), "one");
        assert_eq!(cardinal(Language::English, "1", "0"), "other");
        assert_eq!(cardinal(Language::English, "0", ""), "other");
        assert_eq!(cardinal(Language::English, "2", ""), "other");
    }

    #[test]
    fn english_ordinal() {
        assert_eq!(ordinal("1"), "one");
        assert_eq!(ordinal("2"), "two");
        assert_eq!(ordinal("3"), "few");
        assert_eq!(ordinal("4"), "other");
        assert_eq!(ordinal("11"), "other");
        assert_eq!(ordinal("12"), "other");
        assert_eq!(ordinal("13"), "other");
        assert_eq!(ordinal("21"), "one");
        assert_eq!(ordinal("101"), "one");
        assert_eq!(ordinal("112"), "other");
        assert_eq!(ordinal("1001"), "one");
        assert_eq!(ordinal("22"), "two");
        assert_eq!(ordinal("23"), "few");
    }

    #[test]
    fn categories_per_locale_and_type() {
        let list = |locale: &str, ordinal: bool| icu_plural::categories(locale, ordinal).unwrap();
        assert_eq!(list("en", false), ["one", "other"]);
        assert_eq!(list("en", true), ["one", "two", "few", "other"]);
        assert_eq!(list("pt-BR", false), ["one", "many", "other"]);
        assert_eq!(list("pt-BR", true), ["other"]);
        assert_eq!(list("fr", false), ["one", "many", "other"]);
        assert_eq!(list("ar", false), ["zero", "one", "two", "few", "many", "other"]);
        assert_eq!(list("pl", false), ["one", "few", "many", "other"]);
        assert_eq!(list("ru", false), ["one", "few", "many", "other"]);
        assert_eq!(list("ja", false), ["other"]);
        // Fração visível: o inglês só tem `one` para `i = 1 and v = 0`.
        assert_eq!(icu_plural::select("en", true, "1", "5", 0), Some("other"));
    }

    #[test]
    fn other_locales_cardinal() {
        assert_eq!(select("fr", "0", ""), "one");
        assert_eq!(select("fr", "1", "5"), "one");
        assert_eq!(select("fr", "1000000", ""), "many");
        assert_eq!(select("ar", "0", ""), "zero");
        assert_eq!(select("ar", "2", ""), "two");
        assert_eq!(select("ar", "3", ""), "few");
        assert_eq!(select("ar", "11", ""), "many");
        assert_eq!(select("ar", "100", ""), "other");
        assert_eq!(select("pl", "1", ""), "one");
        assert_eq!(select("pl", "2", ""), "few");
        assert_eq!(select("pl", "5", ""), "many");
        assert_eq!(select("pl", "1", "5"), "other");
        assert_eq!(select("ru", "21", ""), "one");
        assert_eq!(select("ru", "22", ""), "few");
        assert_eq!(select("ru", "25", ""), "many");
        assert_eq!(select("ja", "1", ""), "other");
    }

    #[test]
    fn select_range_uses_ranges_table() {
        // Inglês: qualquer intervalo é `other`; em francês `0..1` é `one` (tabela do CLDR).
        assert_eq!(icu_plural::select_range("en", false, ("0", "", 0), ("1", "", 0)), Some("other"));
        assert_eq!(icu_plural::select_range("fr", false, ("0", "", 0), ("1", "", 0)), Some("one"));
        assert_eq!(icu_plural::select_range("ja", false, ("1", "", 0), ("5", "", 0)), Some("other"));
        // Par ausente da tabela: o ICU devolve `other`, não a categoria da ponta final (`en` ordinal `1..2`,
        // `he` cardinal `1..2`), e o par presente vence (`ru` `0..1` é `one`).
        assert_eq!(icu_plural::select_range("en", true, ("1", "", 0), ("2", "", 0)), Some("other"));
        assert_eq!(icu_plural::select_range("he", false, ("1", "", 0), ("2", "", 0)), Some("other"));
        assert_eq!(icu_plural::select_range("ru", false, ("0", "", 0), ("1", "", 0)), Some("one"));
        // Compacto: `1M` em francês tem `c = 6`, categoria `many`.
        assert_eq!(icu_plural::select("fr", false, "1", "", 6), Some("many"));
    }

    #[test]
    fn portuguese_cardinal() {
        assert_eq!(cardinal(Language::Portuguese, "0", ""), "one");
        assert_eq!(cardinal(Language::Portuguese, "1", "5"), "one");
        assert_eq!(cardinal(Language::Portuguese, "2", ""), "other");
        assert_eq!(cardinal(Language::Portuguese, "1000000", ""), "many");
    }
}
