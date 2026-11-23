// Whether a word is a real word, in Indonesian or English: Upscale Text types a word
// again only when it is (src/convert/retype), so a misread ("paliog") is never drawn
// crisp. Numbers, dates and addresses are judged by the caller.
// - Everywhere: the bundled word list (models/words.txt), the words used at least a few
//   times in a million sentences of each language (Leipzig Corpora Collection).
// - macOS also asks its own spelling dictionaries (NSSpellChecker), for the languages it
//   has one for.

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

fn look_up(word: &str) -> bool {
    in_list(word) || in_system_dictionaries(word)
}

/// The bundled list: lowercase words, one a line
fn in_list(word: &str) -> bool {
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

/// The Mac's own spelling dictionaries, in whichever of Indonesian and English it has.
/// A checker that can't check finds every word right: asked about a language it has no
/// dictionary for, or with no spelling service to ask (as on a headless build machine).
/// So it is only asked about languages it lists, once it has shown it rejects nonsense.
#[cfg(target_os = "macos")]
fn in_system_dictionaries(word: &str) -> bool {
    use std::sync::OnceLock;

    static LANGUAGES: OnceLock<Vec<&'static str>> = OnceLock::new();
    let languages = LANGUAGES.get_or_init(|| {
        objc2::rc::autoreleasepool(|_| {
            let available: Vec<String> =
                objc2_app_kit::NSSpellChecker::sharedSpellChecker().availableLanguages().iter().map(|l| l.to_string()).collect();
            ["id", "en"]
                .into_iter()
                .filter(|language| available.iter().any(|a| a == language))
                .filter(|language| !spelled_right("qzxvbjkwt", language))
                .collect()
        })
    });
    languages.iter().any(|language| spelled_right(word, language))
}

/// Whether the system's checker finds `word` spelled right in `language`
#[cfg(target_os = "macos")]
fn spelled_right(word: &str, language: &str) -> bool {
    use objc2_app_kit::NSSpellChecker;
    use objc2_foundation::{NSNotFound, NSString};

    objc2::rc::autoreleasepool(|_| {
        let wrong = unsafe {
            NSSpellChecker::sharedSpellChecker().checkSpellingOfString_startingAt_language_wrap_inSpellDocumentWithTag_wordCount(
                &NSString::from_str(word),
                0,
                Some(&NSString::from_str(language)),
                false,
                0,
                std::ptr::null_mut(),
            )
        };
        wrong.location == NSNotFound as usize
    })
}

#[cfg(not(target_os = "macos"))]
fn in_system_dictionaries(_word: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    #[test]
    fn real_words_and_misreadings() {
        for word in ["paling", "kecil", "nilai", "Catatan", "diumumkan", "mahasiswa", "Desember", "the", "Following"] {
            assert!(super::known(word), "{word}");
        }
        for word in ["paliog", "nitai", "Calatan", "Desembcr", "tcrlambat", "xqzv"] {
            assert!(!super::known(word), "{word}");
        }
    }

    /// The bundled list alone (all Windows and Linux have; a Mac without dictionaries)
    #[test]
    fn the_word_list_alone_tells_them_apart() {
        for word in ["paling", "kecil", "nilai", "Catatan", "diumumkan", "mahasiswa", "Desember", "Following"] {
            assert!(super::in_list(word), "{word}");
        }
        for word in ["paliog", "nitai", "Calatan", "Desembcr", "tcrlambat"] {
            assert!(!super::in_list(word), "{word}");
        }
    }

    /// A Mac whose checker can't check (no dictionary, no spelling service, as on CI's)
    /// must not take every word for a real one
    #[cfg(target_os = "macos")]
    #[test]
    fn a_missing_dictionary_trusts_nothing() {
        assert!(!super::in_system_dictionaries("xqzvbt"));
    }
}
