use gpui::{App, Global, SharedString};
use icu_locale_core::Locale;
use icu_plurals::{PluralCategory, PluralRules};
use serde::de::{MapAccess, Visitor};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fmt::{self, Write as _},
    rc::Rc,
};

const ENGLISH: &str = "en";

/// The language used by the application.
#[derive(Debug, Default, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[schemars(title = "Language")]
pub struct LanguageSettings {
    /// A BCP-47 language tag. Leave unset to follow the system language.
    pub locale: Option<String>,
}

impl yard_settings::Settings for LanguageSettings {
    const KEY: &'static str = "language";
}

/// Why a catalog could not be registered.
#[derive(Debug)]
pub enum CatalogError {
    BadLocale,
    BadNamespace,
    ReservedNamespace,
    EmptyKey,
    DuplicateKey { key: String },
    InvalidJson(serde_json::Error),
    NotAnObject,
    BadValue { key: String },
    PluralWithoutOther { key: String },
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadLocale => f.write_str("invalid locale tag"),
            Self::BadNamespace => {
                f.write_str("a catalog namespace must be non-empty and have no dot")
            }
            Self::ReservedNamespace => f.write_str("this catalog namespace is reserved"),
            Self::EmptyKey => f.write_str("a catalog key must be non-empty"),
            Self::DuplicateKey { key } => write!(f, "duplicate catalog key {key:?}"),
            Self::InvalidJson(error) => write!(f, "invalid catalog JSON: {error}"),
            Self::NotAnObject => f.write_str("the catalog must be a JSON object"),
            Self::BadValue { key } => write!(f, "invalid catalog value for {key:?}"),
            Self::PluralWithoutOther { key } => write!(f, "plural {key:?} has no 'other' branch"),
        }
    }
}

impl std::error::Error for CatalogError {}

type Messages = HashMap<String, Message>;

enum Message {
    Text(SharedString),
    Plural {
        other: SharedString,
        forms: HashMap<PluralCategory, SharedString>,
    },
}

struct LocaleCatalog {
    rules: Option<PluralRules>,
    namespaces: HashMap<String, Messages>,
}

impl LocaleCatalog {
    fn lookup(&self, namespace: &str, key: &str, count: Option<u64>) -> Option<&SharedString> {
        match self.namespaces.get(namespace)?.get(key)? {
            Message::Text(text) => Some(text),
            Message::Plural { other, forms } => {
                let form = count
                    .zip(self.rules.as_ref())
                    .and_then(|(count, rules)| forms.get(&rules.category_for(count)));
                Some(form.unwrap_or(other))
            }
        }
    }
}

type LocaleChangeListener = Rc<dyn Fn(&mut App)>;

#[derive(PartialEq, Eq)]
struct LocaleRequest {
    source: &'static str,
    tag: Option<String>,
}

struct Catalog {
    catalogs: HashMap<String, LocaleCatalog>,
    active: SharedString,
    parents: Vec<SharedString>,
    initialized: bool,
    system_locale: Option<String>,
    applied_request: Option<LocaleRequest>,
    pending_notice: Option<tracing::Span>,
    reported_unsupported: Option<Locale>,
    missing: RefCell<HashMap<SharedString, HashSet<String>>>,
    listeners: Vec<LocaleChangeListener>,
}

impl Global for Catalog {}

impl Default for Catalog {
    fn default() -> Self {
        Self {
            catalogs: HashMap::new(),
            active: SharedString::new_static(ENGLISH),
            parents: Vec::new(),
            initialized: false,
            system_locale: None,
            applied_request: None,
            pending_notice: None,
            reported_unsupported: None,
            missing: RefCell::default(),
            listeners: Vec::new(),
        }
    }
}

/// Enables system language detection and the editable language section after settings initialization.
pub fn init(cx: &mut App) {
    if cx.default_global::<Catalog>().initialized {
        return;
    }
    let system_locale = (!cx.is_test()).then(sys_locale::get_locale).flatten();
    yard_settings::register_described::<LanguageSettings>(cx);
    let catalog = cx.global_mut::<Catalog>();
    catalog.initialized = true;
    if catalog.system_locale.is_none() {
        catalog.system_locale = system_locale;
    }
    yard_settings::observe::<LanguageSettings>(cx, apply_locale).detach();
    apply_locale(cx);
}

/// Uses an explicit system language in this app's tests. `None` simulates no system language.
pub fn set_system_locale_for_tests(cx: &mut App, system_locale: Option<&str>) {
    cx.default_global::<Catalog>().system_locale = system_locale.map(str::to_owned);
    apply_locale(cx);
}

/// Returns the currently resolved catalog language.
pub fn locale(cx: &App) -> SharedString {
    cx.try_global::<Catalog>()
        .map_or(SharedString::new_static(ENGLISH), |catalog| {
            catalog.active.clone()
        })
}

/// Keeps a callback for this app's lifetime and runs it when the resolved language changes.
pub fn on_locale_change(cx: &mut App, listener: impl Fn(&mut App) + 'static) {
    cx.default_global::<Catalog>()
        .listeners
        .push(Rc::new(listener));
}

/// Resolves the setting (or system language) against the available catalog tags.
pub fn negotiate(requested: Option<&str>, system: Option<&str>, available: &[&str]) -> String {
    requested
        .or(system)
        .and_then(parse_requested_locale)
        .and_then(|locale| {
            match_locale(&locale, |candidate| {
                available.iter().any(|tag| {
                    Locale::try_from_str(tag).is_ok_and(|tag| tag.normalizing_eq(candidate))
                })
            })
        })
        .unwrap_or_else(|| ENGLISH.into())
}

fn parse_requested_locale(tag: &str) -> Option<Locale> {
    let tag = tag.split(['.', '@']).next()?;
    if tag.eq_ignore_ascii_case("C") || tag.eq_ignore_ascii_case("POSIX") {
        return None;
    }
    if tag.contains('_') {
        tag.replace('_', "-").parse().ok()
    } else {
        tag.parse().ok()
    }
}

fn locale_chain(tag: &str) -> impl Iterator<Item = &str> {
    std::iter::successors(Some(tag), |candidate| {
        candidate.rsplit_once('-').map(|(parent, _)| parent)
    })
    // Ignore incomplete extension prefixes, while retaining private-use tags such as `vi-x-a`.
    .filter(|candidate| candidate.parse::<Locale>().is_ok())
}

fn match_locale(locale: &Locale, available: impl Fn(&str) -> bool) -> Option<String> {
    locale_chain(&locale.to_string())
        .find(|tag| available(tag))
        .map(str::to_owned)
}

fn apply_locale(cx: &mut App) {
    let catalog = cx.global::<Catalog>();
    if !catalog.initialized {
        return;
    }
    let setting = yard_settings::get::<LanguageSettings>(cx).locale.as_ref();
    let (source, tag) = setting.map_or(("system", catalog.system_locale.as_ref()), |setting| {
        ("setting", Some(setting))
    });
    let request = LocaleRequest {
        source,
        tag: tag.cloned(),
    };
    let requested = request.tag.as_deref().and_then(parse_requested_locale);
    let matched = requested
        .as_ref()
        .and_then(|locale| match_locale(locale, |tag| catalog.catalogs.contains_key(tag)));
    let resolved = matched.as_deref().unwrap_or(ENGLISH);
    let changed = catalog.active != resolved;
    if !changed && catalog.applied_request.as_ref() == Some(&request) {
        return;
    }

    let group = tracing::info_span!(
        "locale-apply",
        requested = request.tag.as_deref(),
        resolved,
        source
    );
    let _entered = group.enter();
    let catalog = cx.global_mut::<Catalog>();
    catalog.applied_request = Some(request);
    if changed {
        catalog.active = SharedString::from(resolved);
        catalog.parents = locale_chain(resolved)
            .skip(1)
            .map(SharedString::from)
            .collect();
    }
    let unsupported =
        matched.is_none() && requested.is_some_and(|tag| tag.id.language.as_str() != ENGLISH);
    // Startup can register catalogs in the same update as initialization. Check only
    // the final request and carry the apply span into the deferred work.
    if (unsupported || catalog.reported_unsupported.is_some())
        && catalog.pending_notice.replace(group.clone()).is_none()
    {
        cx.defer(report_missing_catalog);
    }
    if changed {
        let catalog = cx.global::<Catalog>();
        for listener in catalog.listeners.clone() {
            listener(cx);
        }
    }
    cx.refresh_windows();
    tracing::debug!("applied");
}

fn report_missing_catalog(cx: &mut App) {
    let catalog = cx.global_mut::<Catalog>();
    let Some(group) = catalog.pending_notice.take() else {
        return;
    };
    let Some(request) = &catalog.applied_request else {
        return;
    };
    let unsupported = request
        .tag
        .as_deref()
        .and_then(parse_requested_locale)
        .filter(|tag| {
            tag.id.language.as_str() != ENGLISH
                && match_locale(tag, |tag| catalog.catalogs.contains_key(tag)).is_none()
        });
    if unsupported.is_some() && unsupported != catalog.reported_unsupported {
        let _entered = group.enter();
        tracing::info!(
            requested = request.tag.as_deref(),
            source = request.source,
            "no catalog for requested language; using English"
        );
    }
    catalog.reported_unsupported = unsupported;
}

/// Registers one JSON catalog, replacing the previous catalog for this namespace and locale.
pub fn register(
    cx: &mut App,
    namespace: &str,
    locale: &str,
    json: &str,
) -> Result<(), CatalogError> {
    let (tag, messages) = match parse_catalog(namespace, locale, json) {
        Ok(catalog) => catalog,
        Err(error) => {
            let _group = tracing::info_span!("catalog-register", namespace, locale).entered();
            log::error!("catalog {namespace:?} for {locale:?} refused: {error}");
            return Err(error);
        }
    };
    let group = tracing::info_span!("catalog-register", namespace, locale, keys = messages.len());
    {
        let _entered = group.enter();
        cx.default_global::<Catalog>()
            .catalogs
            .entry(tag.to_string())
            .or_insert_with(|| LocaleCatalog {
                rules: PluralRules::try_new_cardinal((&tag).into()).ok(),
                namespaces: HashMap::new(),
            })
            .namespaces
            .insert(namespace.into(), messages);
        tracing::debug!("registered");
    }
    apply_locale(cx);
    cx.refresh_windows();
    Ok(())
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum CatalogValue {
    Text(String),
    Object(Members),
    Other(serde::de::IgnoredAny),
}

// Keep duplicate detection as data across untagged deserialization attempts.
struct Members(Result<HashMap<String, CatalogValue>, String>);

impl<'de> serde::Deserialize<'de> for Members {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ObjectVisitor;

        impl<'de> Visitor<'de> for ObjectVisitor {
            type Value = Members;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an object")
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Members, M::Error> {
                use std::collections::hash_map::Entry;

                let mut members = HashMap::<String, CatalogValue>::new();
                let mut duplicate = None;
                while let Some((key, value)) = map.next_entry()? {
                    match members.entry(key) {
                        Entry::Vacant(entry) => {
                            entry.insert(value);
                        }
                        Entry::Occupied(entry) => {
                            duplicate.get_or_insert_with(|| entry.key().clone());
                        }
                    }
                }
                Ok(Members(duplicate.map_or(Ok(members), Err)))
            }
        }

        deserializer.deserialize_map(ObjectVisitor)
    }
}

fn parse_catalog(
    namespace: &str,
    locale: &str,
    json: &str,
) -> Result<(Locale, Messages), CatalogError> {
    if namespace.is_empty() || namespace.contains('.') {
        return Err(CatalogError::BadNamespace);
    }
    let locale = Locale::try_from_str(locale).map_err(|_| CatalogError::BadLocale)?;
    let CatalogValue::Object(Members(entries)) =
        serde_json::from_str(json).map_err(CatalogError::InvalidJson)?
    else {
        return Err(CatalogError::NotAnObject);
    };
    let entries = entries.map_err(|key| CatalogError::DuplicateKey { key })?;
    let mut messages = HashMap::with_capacity(entries.len());
    for (key, value) in entries {
        if key.is_empty() {
            return Err(CatalogError::EmptyKey);
        }
        let message = parse_message(&key, value)?;
        messages.insert(key, message);
    }
    Ok((locale, messages))
}

fn parse_message(key: &str, value: CatalogValue) -> Result<Message, CatalogError> {
    let bad_value = || CatalogError::BadValue { key: key.into() };
    let entries = match value {
        CatalogValue::Text(text) => return Ok(Message::Text(text.into())),
        CatalogValue::Object(Members(entries)) => {
            entries.map_err(|key| CatalogError::DuplicateKey { key })?
        }
        CatalogValue::Other(_) => return Err(bad_value()),
    };
    let mut forms = HashMap::new();
    for (name, value) in entries {
        let category = PluralCategory::get_for_cldr_string(&name).ok_or_else(bad_value)?;
        let CatalogValue::Text(text) = value else {
            return Err(bad_value());
        };
        forms.insert(category, text.into());
    }
    let other = forms
        .remove(&PluralCategory::Other)
        .ok_or_else(|| CatalogError::PluralWithoutOther { key: key.into() })?;
    Ok(Message::Plural { other, forms })
}

/// Looks up a key in the current language, falling back to English and then the key.
pub fn t(cx: &App, key: &str) -> SharedString {
    lookup(cx, key, None).cloned().unwrap_or_else(|| key.into())
}

/// Looks up a key and substitutes the supplied named arguments.
pub fn t_with(cx: &App, key: &str, args: &[(&str, &str)]) -> SharedString {
    lookup(cx, key, None)
        .map(|text| interpolate(text, args, None))
        .unwrap_or_else(|| key.into())
}

/// Selects a cardinal plural form and substitutes `{count}`.
pub fn t_count(cx: &App, key: &str, count: u64) -> SharedString {
    lookup(cx, key, Some(count))
        .map(|text| interpolate(text, &[], Some(count)))
        .unwrap_or_else(|| key.into())
}

fn lookup<'a>(cx: &'a App, key: &str, count: Option<u64>) -> Option<&'a SharedString> {
    let Some(catalog) = cx.try_global::<Catalog>() else {
        tracing::warn!(locale = ENGLISH, key, "missing translation");
        return None;
    };
    let parts = key.split_once('.');
    let find = |locale: &str| {
        let (namespace, key) = parts?;
        catalog.catalogs.get(locale)?.lookup(namespace, key, count)
    };
    for locale in std::iter::once(&catalog.active).chain(&catalog.parents) {
        let Some(text) = find(locale) else { continue };
        tracing::trace!(%locale, key, "translated");
        return Some(text);
    }

    let first_miss = {
        let mut missing = catalog.missing.borrow_mut();
        let keys = missing.entry(catalog.active.clone()).or_default();
        !keys.contains(key) && keys.insert(key.into())
    };
    if first_miss {
        log::warn!("missing translation for {key:?} in {}", catalog.active);
    } else {
        tracing::warn!(locale = %catalog.active, key, "missing translation");
    }
    find(ENGLISH)
}

fn interpolate(text: &SharedString, args: &[(&str, &str)], count: Option<u64>) -> SharedString {
    if args.is_empty() && count.is_none() {
        return text.clone();
    }
    let mut rendered = None;
    let mut remaining = text.as_str();
    let mut copied = 0;
    while let Some((_, after_open)) = remaining.split_once('{') {
        let Some((name, after_close)) = after_open.split_once('}') else {
            break;
        };
        // If braces nest, a placeholder can only start at the innermost opening brace.
        let name = name.rsplit('{').next().unwrap_or(name);
        remaining = after_close;
        let count = count.filter(|_| name == "count");
        let arg = args
            .iter()
            .find_map(|&(key, value)| (key == name).then_some(value));
        if count.is_none() && arg.is_none() {
            continue;
        }
        let end = text.len() - remaining.len();
        let start = end - name.len() - 2;
        let output = rendered.get_or_insert_with(|| String::with_capacity(text.len()));
        output.push_str(&text[copied..start]);
        if let Some(count) = count {
            write!(output, "{count}").expect("writing to a string cannot fail");
        } else if let Some(arg) = arg {
            output.push_str(arg);
        }
        copied = end;
    }
    rendered.map_or_else(
        || text.clone(),
        |mut output| {
            output.push_str(&text[copied..]);
            output.into()
        },
    )
}
