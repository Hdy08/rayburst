/**
 * @fileoverview Locale loading and DOM translations for the Rayburst website.
 *
 * Architecture:
 *   1. Detect language: URL hash (#lang=xx) > localStorage > navigator.languages > en-US
 *   2. Fetch JSON locale file from /locales/{lang}.json
 *   3. Walk DOM for [data-i18n] attributes and replace textContent
 *   4. Handle interpolation: {variable} placeholders
 *   5. RTL support for Arabic and Persian
 *
 * Zero dependencies.
 */

const SUPPORTED_LOCALES = [
  'ar',
  'bg',
  'ca',
  'de',
  'el',
  'en-US',
  'es',
  'fa',
  'fr',
  'hu',
  'hi',
  'id',
  'it',
  'ja',
  'ko',
  'nb',
  'nl',
  'pl',
  'pt-BR',
  'ro',
  'ru',
  'th',
  'tr',
  'uk',
  'vi',
  'zh-CN',
  'zh-TW',
]

/** Native display names for the toggle button (always in the locale's own language). */
const LOCALE_NAMES = {
  ar: 'العربية',
  bg: 'Български',
  ca: 'Català',
  de: 'Deutsch',
  el: 'Ελληνικά',
  'en-US': 'English',
  es: 'Español',
  fa: 'فارسی',
  fr: 'Français',
  hu: 'Magyar',
  hi: 'हिन्दी',
  id: 'Indonesia',
  it: 'Italiano',
  ja: '日本語',
  ko: '한국어',
  nb: 'Norsk',
  nl: 'Nederlands',
  pl: 'Polski',
  'pt-BR': 'Português',
  ro: 'Română',
  ru: 'Русский',
  th: 'ไทย',
  tr: 'Türkçe',
  uk: 'Українська',
  vi: 'Tiếng Việt',
  'zh-CN': '简体中文',
  'zh-TW': '繁體中文',
}

const RTL_LOCALES = ['ar', 'fa']
const FALLBACK = 'en-US'
const STORAGE_KEY = 'rayburst-website-lang'

let currentLocale = FALLBACK
let detectedSystemLocale = null
let localeRequest = null
const fallbackMessages = readFallbackMessages()
let messages = fallbackMessages
const textElements = document.querySelectorAll('[data-i18n], [data-i18n-html], [data-locale-label]')

/** Registered callbacks invoked after every locale switch. */
const localeChangeCallbacks = []

/** The static page also provides English when every locale request fails. */
function readFallbackMessages() {
  const result = {}
  for (const root of [document, document.getElementById('locale-fallback').content]) {
    root.querySelectorAll('[data-i18n]').forEach((el) => {
      result[el.dataset.i18n] = el.textContent.trim().replace(/\s+/g, ' ')
    })
    root.querySelectorAll('[data-i18n-html]').forEach((el) => {
      result[el.dataset.i18nHtml] = el.innerHTML.trim().replace(/\s+/g, ' ')
    })
    for (const attribute of ['alt', 'aria-label']) {
      root.querySelectorAll(`[data-i18n-${attribute}]`).forEach((el) => {
        result[el.getAttribute(`data-i18n-${attribute}`)] = el.getAttribute(attribute)
      })
    }
  }
  return result
}

/** Placeholder controls must not remain keyboard targets without visible labels. */
function setTextPending(pending) {
  document.documentElement.toggleAttribute('data-locale-pending', pending)
  document.documentElement.setAttribute('aria-busy', String(pending))
  for (const el of textElements) el.inert = pending
}

/** Resolve the best locale from browser language, e.g. "zh-CN" or "zh" → "zh-CN". */
function resolveLocale(raw) {
  if (!raw) return null
  const normalized = raw.trim()
  if (SUPPORTED_LOCALES.includes(normalized)) return normalized
  // Region-specific overrides (Traditional Chinese regions → zh-TW)
  const REGION_MAP = { 'zh-hk': 'zh-TW', 'zh-mo': 'zh-TW', 'zh-tw': 'zh-TW' }
  const lower = normalized.toLowerCase()
  if (REGION_MAP[lower]) return REGION_MAP[lower]
  // Try base language match: "zh" → "zh-CN", "pt" → "pt-BR"
  const base = normalized.split('-')[0].toLowerCase()
  const match = SUPPORTED_LOCALES.find((l) => l.toLowerCase().startsWith(base))
  return match || null
}

/** Detect preferred locale from URL hash > localStorage > navigator. */
function detectLocale() {
  // 1. URL hash: #lang=zh-CN
  const hash = location.hash.match(/lang=([^&]+)/)
  if (hash) {
    const resolved = resolveLocale(hash[1])
    if (resolved) return resolved
  }
  // 2. localStorage
  let stored
  try {
    stored = localStorage.getItem(STORAGE_KEY)
  } catch {
    // Language detection does not require persistent storage.
  }
  if (stored && SUPPORTED_LOCALES.includes(stored)) return stored
  // Respect the browser's ordered language preferences.
  for (const lang of navigator.languages) {
    const resolved = resolveLocale(lang)
    if (resolved) return resolved
  }
  return FALLBACK
}

/** Fetch only the requested language; failures use the static English copy. */
async function fetchLocale(locale, signal) {
  try {
    const res = await fetch(new URL(`locales/${locale}.json`, document.baseURI), {
      signal,
    })
    if (!res.ok) return null
    const data = await res.json()
    if (!data || Array.isArray(data) || typeof data !== 'object') return null
    const entries = Object.entries(data).filter(
      ([key, value]) => Object.hasOwn(fallbackMessages, key) && typeof value === 'string' && value.trim(),
    )
    return entries.length ? Object.fromEntries(entries) : null
  } catch {
    return null
  }
}

/** Interpolate {variable} placeholders in a string. */
function interpolate(template, vars) {
  if (!vars || !template) return template
  return template.replace(/\{(\w+)\}/g, (_, key) => vars[key] ?? `{${key}}`)
}

/** Get a translated string by key, with optional interpolation variables. */
function t(key, vars) {
  const raw = messages[key] ?? fallbackMessages[key] ?? key
  return vars ? interpolate(raw, vars) : raw
}

/** Apply translations to all [data-i18n] elements in the DOM. */
function applyTranslations() {
  document.querySelectorAll('[data-i18n]').forEach((el) => {
    const key = el.getAttribute('data-i18n')
    if (key && (key in messages || key in fallbackMessages)) el.textContent = t(key)
  })
  // HTML interpolation variables — keeps locale files free of markup
  const HTML_VARS = {
    aria2Next: '<a href="https://github.com/AnInsomniacy/aria2-next" target="_blank" rel="noopener">Aria2 Next</a>',
  }
  document.querySelectorAll('[data-i18n-html]').forEach((el) => {
    const key = el.getAttribute('data-i18n-html')
    if (key && (key in messages || key in fallbackMessages)) el.innerHTML = t(key, HTML_VARS)
  })
  for (const attribute of ['alt', 'aria-label']) {
    document.querySelectorAll(`[data-i18n-${attribute}]`).forEach((el) => {
      const key = el.getAttribute(`data-i18n-${attribute}`)
      if (key && (key in messages || key in fallbackMessages)) el.setAttribute(attribute, t(key))
    })
  }

  // Update HTML lang and dir attributes
  document.documentElement.lang = currentLocale
  document.documentElement.dir = RTL_LOCALES.includes(currentLocale) ? 'rtl' : 'ltr'

  // Update active state in language picker
  document.querySelectorAll('.picker-option[data-lang]').forEach((opt) => {
    opt.classList.toggle('active', opt.dataset.lang === currentLocale)
  })

  // Update toggle button to show current language name
  const toggleLabel = document.getElementById('lang-toggle-label')
  if (toggleLabel) toggleLabel.textContent = LOCALE_NAMES[currentLocale] || currentLocale

  // Update system detection hint in dropdown
  const sysHint = document.getElementById('lang-system-hint')
  if (sysHint && detectedSystemLocale) {
    const sysName = LOCALE_NAMES[detectedSystemLocale] || detectedSystemLocale
    sysHint.textContent = `${t('theme.system')}: ${sysName}`
    sysHint.dataset.lang = detectedSystemLocale
    sysHint.style.display = ''
    const sysSep = document.getElementById('lang-system-sep')
    if (sysSep) sysSep.style.display = ''
  }
}

/** Resolve a language before revealing text; only the latest request may commit. */
async function setLocale(locale, persist = true) {
  if (!SUPPORTED_LOCALES.includes(locale)) return
  const root = document.documentElement
  if (locale === currentLocale && !root.hasAttribute('data-locale-pending')) return
  localeRequest?.abort()
  const controller = new AbortController()
  localeRequest = controller
  window.localeBoot.finish()
  root.removeAttribute('data-locale-reveal')
  setTextPending(true)
  const started = performance.now()

  try {
    const localized =
      locale === FALLBACK
        ? fallbackMessages
        : await fetchLocale(locale, AbortSignal.any([controller.signal, AbortSignal.timeout(8000)]))
    if (localeRequest !== controller) return

    messages = localized || fallbackMessages
    currentLocale = localized ? locale : FALLBACK
    applyTranslations()
    for (const cb of localeChangeCallbacks) cb()

    if (persist && localized) {
      try {
        localStorage.setItem(STORAGE_KEY, currentLocale)
      } catch {
        // Translation still works when storage is unavailable.
      }
      location.hash = `lang=${currentLocale}`
    }
  } finally {
    if (localeRequest === controller) {
      localeRequest = null
      setTextPending(false)
      if (performance.now() - started > 100) root.setAttribute('data-locale-reveal', '')
    }
  }
}

/** Register a callback to re-render dynamic content on locale change. */
function onLocaleChange(cb) {
  localeChangeCallbacks.push(cb)
}

/** Initialize i18n: detect language, load messages, render. */
async function initI18n() {
  detectedSystemLocale = navigator.languages.map(resolveLocale).find(Boolean) || FALLBACK
  // If the bootstrap expired before this script arrived, keep its English fallback.
  if (!window.localeBoot.finished) await setLocale(detectLocale(), false)
}

// Expose globally for inline usage
window.i18n = { t, setLocale, onLocaleChange, currentLocale: () => currentLocale, SUPPORTED_LOCALES }
// Only translated text waits; branding, images and layout render independently.
window.i18n.ready = initI18n()
