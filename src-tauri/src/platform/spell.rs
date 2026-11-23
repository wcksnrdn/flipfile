// Whether a word is a real word, in Indonesian or English: Upscale Text types a word
// again only when it is (src/convert/retype), so a misread ("paliog") is never drawn
// crisp. Numbers, dates and addresses are judged by the caller.
// - macOS: the system's own spelling dictionaries (NSSpellChecker), which know both.
// - Windows and Linux: the bundled word list (models/words.txt), the words used at least
//   a few times in a million sentences of each language (Leipzig Corpora Collection).

use std::collections::HashMap;
use std::sync::Mutex;

/// What has been looked up, so a word is asked about once
static SEEN: Mutex<Option<HashMap<String, bool>>> = Mutex::new(None);

/// Whether `word` (letters only, any case) is an Indonesian or English word
pub fn known(word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    let seen = seen.get_or_insert_with(HashMap::new);
    if let Some(&known) = seen.get(word) {
        return known;
    }
    let known = look_up(word) || (word.chars().any(char::is_uppercase) && look_up(&word.to_lowercase()));
    seen.insert(word.to_string(), known);
    known
}

#[cfg(target_os = "macos")]
fn look_up(word: &str) -> bool {
    use objc2_app_kit::NSSpellChecker;
    use objc2_foundation::{NSNotFound, NSString};

    objc2::rc::autoreleasepool(|_| {
        let checker = NSSpellChecker::sharedSpellChecker();
        let text = NSString::from_str(word);
        ["id", "en"].iter().any(|language| {
            let wrong = unsafe {
                checker.checkSpellingOfString_startingAt_language_wrap_inSpellDocumentWithTag_wordCount(
                    &text,
                    0,
                    Some(&NSString::from_str(language)),
                    false,
                    0,
                    std::ptr::null_mut(),
                )
            };
            wrong.location == NSNotFound as usize
        })
    })
}

#[cfg(not(target_os = "macos"))]
fn look_up(word: &str) -> bool {
    use std::collections::HashSet;
    use std::sync::OnceLock;

    static WORDS: OnceLock<HashSet<String>> = OnceLock::new();
    WORDS
        .get_or_init(|| {
            super::upscale::model_path("words.txt")
                .and_then(|path| std::fs::read_to_string(path).ok())
                .map(|list| list.lines().map(str::to_string).collect())
                .unwrap_or_default()
        })
        .contains(&word.to_lowercase())
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    #[test]
    fn real_words_and_misreadings() {
        for word in ["paling", "kecil", "nilai", "Catatan", "diumumkan", "mahasiswa", "Desember", "the", "Following"] {
            assert!(super::known(word), "{word}");
        }
        for word in ["paliog", "nitai", "Calatan", "Desembcr", "tcrlambat", "xqzv"] {
            assert!(!super::known(word), "{word}");
        }
    }
}
