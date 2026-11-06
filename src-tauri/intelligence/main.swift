// flipfile-ai: flipfile's bridge to the on-device Apple Intelligence model.
//
// A small helper the app starts on demand (src/summary/helper.rs), so the main binary
// never links Swift or FoundationModels and keeps launching on every macOS version.
// Everything stays on this Mac: FoundationModels runs Apple's on-device model.
//
//   flipfile-ai status      one JSON line: {"available":true} or {"available":false,"reason":"..."}
//   flipfile-ai warm        loads the model ahead of a summary, so it starts writing sooner
//   flipfile-ai summarize   reads one JSON request on stdin, writes events as JSON lines on stdout
//                           (a summary, a translation of one, or an answer to a question)
//
// The model sees 4096 tokens at once: instructions, document and answer together. Short
// documents are summarised in one go. Longer ones are first read part by part into notes,
// and very long ones are condensed to their most telling sentences before that, so even a
// book takes about a minute (the model answers one request at a time, ~7 s per part).

import Foundation
import FoundationModels
import NaturalLanguage

// MARK: - Events

/// Writes one event as a JSON line; the app reads stdout line by line.
func emit(_ event: String, _ fields: [String: Any] = [:]) {
    var object = fields
    object["event"] = event
    guard JSONSerialization.isValidJSONObject(object),
          var data = try? JSONSerialization.data(withJSONObject: object, options: [.withoutEscapingSlashes])
    else { return }
    data.append(0x0A)
    FileHandle.standardOutput.write(data)
}

/// Streams snapshots at most every `interval`, so the app is not flooded with
/// hundreds of near-identical events while the model writes.
struct Throttle {
    let interval: Duration
    var last = ContinuousClock.now - .seconds(1)

    mutating func ready() -> Bool {
        let now = ContinuousClock.now
        guard now - last >= interval else { return false }
        last = now
        return true
    }
}

// MARK: - Availability

/// Why the model can't be used right now, or nil when it can
func unavailableReason() -> String? {
    switch SystemLanguageModel.default.availability {
    case .available:
        return nil
    case .unavailable(let reason):
        switch reason {
        case .deviceNotEligible: return "deviceNotEligible"
        case .appleIntelligenceNotEnabled: return "appleIntelligenceNotEnabled"
        case .modelNotReady: return "modelNotReady"
        @unknown default: return "unavailable"
        }
    }
}

// MARK: - Errors

enum Failure: String, Error {
    case context, guardrail, refusal, language, assets, busy, empty, request, other
}

/// Sorts the framework's errors into what the summary can do about them. macOS 26 throws
/// `LanguageModelSession.GenerationError`, macOS 27 `LanguageModelError` (whatever SDK
/// the helper was built with), and the latter only exists in the macOS 27 SDK; matching
/// their cases by name lets one build, from either SDK, understand both.
func classify(_ error: Error) -> Failure {
    if let failure = error as? Failure { return failure }
    let name = Mirror(reflecting: error).children.first?.label ?? String(describing: error)
    switch name.prefix(while: { $0 != "(" }) {
    case "contextSizeExceeded", "exceededContextWindowSize": return .context
    case "guardrailViolation": return .guardrail
    case "refusal": return .refusal
    case "unsupportedLanguageOrLocale": return .language
    case "assetsUnavailable": return .assets
    case "rateLimited", "concurrentRequests": return .busy
    default: return .other
    }
}

// MARK: - Text

/// The sentences of `text`, in any language NaturalLanguage knows
func sentences(of text: String) -> [String] {
    let tokenizer = NLTokenizer(unit: .sentence)
    tokenizer.string = text
    return tokenizer.tokens(for: text.startIndex..<text.endIndex)
        .map { text[$0].trimmingCharacters(in: .whitespacesAndNewlines) }
        .filter { !$0.isEmpty }
}

/// Cuts text into pieces of at most `limit` characters: at paragraph ends where possible,
/// else at sentence ends, else anywhere.
func split(_ text: String, limit: Int) -> [String] {
    var pieces: [String] = []
    var current = ""
    var count = 0
    func add(_ part: String, _ length: Int, separator: String) {
        if count > 0 && count + length + 1 > limit {
            pieces.append(current)
            current = ""
            count = 0
        }
        current += count == 0 ? part : separator + part
        count += length + (count == 0 ? 0 : 1)
    }
    for line in text.split(separator: "\n", omittingEmptySubsequences: true) {
        let paragraph = String(line)
        let length = paragraph.count
        if length <= limit {
            add(paragraph, length, separator: "\n")
            continue
        }
        for sentence in sentences(of: paragraph) {
            var rest = Substring(sentence)
            while rest.count > limit {
                add(String(rest.prefix(limit)), limit, separator: " ")
                rest = rest.dropFirst(limit)
            }
            add(String(rest), rest.count, separator: " ")
        }
    }
    if count > 0 { pieces.append(current) }
    return pieces
}

/// Passages to answer questions from: the text cut at its headings (and every `limit`
/// characters), each led by the headings it sits under, so the "Fixed" list under
/// "[0.5.2]" is still found by a question about 0.5.2.
func passages(_ text: String, limit: Int) -> [String] {
    var trail: [(level: Int, title: String)] = []
    var out: [String] = []
    var body: [String] = []
    var size = 0
    func flush() {
        guard !body.isEmpty else { return }
        let header = trail.map(\.title).joined(separator: " › ")
        out.append((header.isEmpty ? "" : header + "\n") + body.joined(separator: "\n"))
        body = []
        size = 0
    }
    for line in text.split(separator: "\n", omittingEmptySubsequences: true) {
        let paragraph = String(line)
        if paragraph.hasPrefix("#") {
            flush()
            let level = paragraph.prefix(while: { $0 == "#" }).count
            trail.removeAll { $0.level >= level }
            trail.append((level, paragraph.drop(while: { $0 == "#" }).trimmingCharacters(in: .whitespaces)))
            continue
        }
        for piece in paragraph.count > limit ? split(paragraph, limit: limit) : [paragraph] {
            if size > 0 && size + piece.count > limit { flush() }
            body.append(piece)
            size += piece.count + 1
        }
    }
    flush()
    return out
}

struct Sentence {
    let text: String
    let paragraph: Int
    let opening: Bool
    let heading: Bool
    let words: [String]
}

/// The sentences of a text with how much each tells about it.
///
/// A sentence tells more when it has words that are frequent in the document but not
/// everywhere (names, topics, figures rather than "the" or "dan"); headings, numbers and
/// paragraph openings get a small bonus.
func scored(_ text: String) -> (list: [Sentence], scores: [Double]) {
    var list: [Sentence] = []
    for (index, line) in text.split(separator: "\n", omittingEmptySubsequences: true).enumerated() {
        let paragraph = String(line)
        let words = { (s: String) in
            s.lowercased().split(whereSeparator: { !$0.isLetter && !$0.isNumber }).filter { $0.count >= 3 }.map(String.init)
        }
        if paragraph.hasPrefix("#") {
            list.append(Sentence(text: paragraph, paragraph: index, opening: true, heading: true, words: words(paragraph)))
            continue
        }
        for (position, sentence) in sentences(of: paragraph).enumerated() {
            list.append(Sentence(text: sentence, paragraph: index, opening: position == 0, heading: false, words: words(sentence)))
        }
    }
    guard !list.isEmpty else { return ([], []) }

    var frequency: [String: Int] = [:]
    var spread: [String: Int] = [:]
    for sentence in list {
        for word in sentence.words { frequency[word, default: 0] += 1 }
        for word in Set(sentence.words) { spread[word, default: 0] += 1 }
    }
    let total = Double(list.count)
    let scores: [Double] = list.map { sentence in
        let weight = sentence.words.reduce(0.0) { sum, word in
            sum + log(1 + Double(frequency[word] ?? 0)) * log(total / Double(spread[word] ?? 1))
        }
        var score = weight / Double(max(sentence.words.count, 6)).squareRoot()
        if sentence.heading { score *= 1.6 }
        if sentence.opening { score *= 1.15 }
        if sentence.text.contains(where: \.isNumber) { score *= 1.15 }
        return score
    }
    return (list, scores)
}

/// Keeps the most telling sentences of a very long text, about `target` characters, in
/// their original order. The text is cut into `windows` stretches that each keep their
/// share, so the end of a book is represented as well as its start.
func condense(_ text: String, target: Int, windows: Int) -> String {
    let (list, scores) = scored(text)
    guard !list.isEmpty else { return "" }

    let lengths = list.map { $0.text.count + 1 }
    let all = lengths.reduce(0, +)
    let stretch = max(1, all / max(windows, 1))
    let share = max(1, target / max(windows, 1))
    var keep = [Bool](repeating: false, count: list.count)
    var start = 0
    while start < list.count {
        var end = start
        var size = 0
        while end < list.count && (size < stretch || end == start) {
            size += lengths[end]
            end += 1
        }
        var used = 0
        for index in (start..<end).sorted(by: { scores[$0] > scores[$1] }) where used + lengths[index] <= share || used == 0 {
            keep[index] = true
            used += lengths[index]
        }
        start = end
    }

    var out = ""
    var paragraph = -1
    for (index, sentence) in list.enumerated() where keep[index] {
        if paragraph >= 0 { out += sentence.paragraph == paragraph ? " " : "\n" }
        out += sentence.text
        paragraph = sentence.paragraph
    }
    return out
}

/// A first look at a document while the model is still reading it: `count` of its most
/// telling sentences, picked in a moment without the model and shown in their original
/// order. A document mostly says what it is about early on (an abstract, an
/// introduction, the first lines of a letter), so earlier sentences weigh more. Only
/// full statements of a readable length make a good preview: no headings, questions,
/// table rows or lines cut off by a page break.
func peek(_ text: String, count: Int) -> [String] {
    // A book takes a while to tokenize, and its opening tells the most anyway
    let (list, scores) = scored(String(text.prefix(60_000)))
    let readable = list.indices.filter { index in
        let sentence = list[index]
        let words = sentence.text.split(separator: " ")
        let shouting = words.filter { $0.count > 1 && $0.allSatisfy { !$0.isLowercase } && $0.contains(where: \.isLetter) }
        return !sentence.heading
            && (40...280).contains(sentence.text.count)
            && sentence.words.count >= 6
            // A full statement: it starts as a sentence and ends as one
            && plain(sentence.text).first.map { !$0.isLowercase && !$0.isPunctuation } == true
            && sentence.text.last.map { ".!…。！\"”".contains($0) } == true
            // Not a table row, a letterhead or a list of codes and names
            && shouting.count * 4 < words.count
            && !sentence.text.contains("_") && !sentence.text.contains("|")
            && sentence.text.filter({ !$0.isLetter && !$0.isWhitespace }).count * 5 < sentence.text.count
            // Not run together with a heading ("BAB II LANDASAN TEORI 2.1 Rekayasa ...")
            && sentence.text.range(of: #"(^|\s)\d+(\.\d+)+\s+\p{Lu}|^\p{Lu}{2,}\s+\p{Lu}{2,}\s|\p{Lu}{2,}\s+\p{Lu}{2,}\s+\p{Lu}{2,}"#, options: .regularExpression) == nil
            // Not cut off from what it belongs to: brackets that close what they open
            && sentence.text.filter({ $0 == "(" }).count == sentence.text.filter({ $0 == ")" }).count
            && sentence.text.range(of: #"\s\p{L}\.$"#, options: .regularExpression) == nil
    }
    guard readable.count > count else { return readable.map { plain(list[$0].text) } }

    let weight = { (index: Int) in scores[index] * (1.6 - Double(index) / Double(list.count)) }
    var picked: [Int] = []
    // Best first, but never two neighbours: three different things to see
    for index in readable.sorted(by: { weight($0) > weight($1) }) where picked.count < count {
        if picked.allSatisfy({ abs($0 - index) > 2 }) { picked.append(index) }
    }
    return picked.sorted().map { plain(list[$0].text) }
}

/// A sentence without its Markdown: list marks, emphasis, code ticks and link targets
func plain(_ sentence: String) -> String {
    var text = sentence
    text = text.replacingOccurrences(of: #"^\s*(?:[-*+•>]|\d+[.)])\s+"#, with: "", options: .regularExpression)
    text = text.replacingOccurrences(of: #"!?\[([^\]]*)\]\([^)]*\)"#, with: "$1", options: .regularExpression)
    text = text.replacingOccurrences(of: #"\*\*|__|`"#, with: "", options: .regularExpression)
    text = text.replacingOccurrences(of: #"(?<![\w*])\*([^*\s][^*]*)\*"#, with: "$1", options: .regularExpression)
    return text.trimmingCharacters(in: .whitespaces)
}

/// The document's main language ("id", "en", ...), judged from its first 20 000 characters
func dominantLanguage(of text: String) -> String? {
    let recognizer = NLLanguageRecognizer()
    recognizer.processString(String(text.prefix(20_000)))
    return recognizer.dominantLanguage?.rawValue
}

/// "id" -> "Indonesian": the model follows a language named in English best
func languageName(_ code: String) -> String {
    Locale(identifier: "en_US").localizedString(forLanguageCode: code) ?? "English"
}

// MARK: - Request

struct Request: Decodable {
    var text: String
    var name: String?
    var kind: String?
    var pages: Int?
    /// "auto" (the document's own language) or a language code
    var language: String?
    /// Notes from an earlier run of the same document: only the summary is written again
    var notes: [String]?
    /// A finished summary (JSON) to translate into `language` instead of reading `text`
    var summary: String?
    /// A question to answer from `text` instead of summarizing it
    var question: String?
}

// MARK: - Summarizer

/// Token budgets inside the 4096-token window
enum Budget {
    /// A document this long is read in one go, next to the summary schema (~450) and the answer (~800)
    static let single = 2_300
    /// One part read into notes (instructions ~120, notes up to 320)
    static let part = 2_900
    /// At most this many parts (~7 s each); longer documents are condensed first
    static let parts = 8
}

final class Summarizer {
    let model = SystemLanguageModel(useCase: .general, guardrails: .permissiveContentTransformations)
    let request: Request
    var language: String
    let documentLanguage: String?

    init(_ request: Request) {
        self.request = request
        documentLanguage = dominantLanguage(of: request.text)
        let wanted = request.language ?? "auto"
        language = wanted == "auto" ? (documentLanguage ?? "en") : wanted
    }

    /// File name, kind and length, so the model knows what it is looking at
    var meta: String {
        var parts = ["File: \"\(request.name ?? "Document")\""]
        if let kind = request.kind { parts.append(kind) }
        if let pages = request.pages, pages > 1 { parts.append("\(pages) pages") }
        return parts.joined(separator: " · ")
    }

    func run() async throws {
        let started = ContinuousClock.now
        emit("language", ["document": documentLanguage ?? NSNull(), "output": language])

        if let question = request.question?.trimmingCharacters(in: .whitespacesAndNewlines), !question.isEmpty {
            try await answer(question)
            return
        }
        if let json = request.summary, let data = json.data(using: .utf8),
           let summary = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] {
            emit("compose")
            let translated = try await translate(summary)
            emit("done", ["data": translated, "notes": request.notes ?? [], "language": language, "seconds": elapsed(since: started)])
            return
        }

        let text = request.text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { throw Failure.empty }

        var notes = request.notes ?? []
        var source = text
        var fromNotes = !notes.isEmpty
        if !fromNotes {
            // Something to read at once, while the model is still getting to it
            let preview = peek(text, count: 3)
            if !preview.isEmpty { emit("peek", ["sentences": preview]) }

            // A short text fits even at the densest (one character a token): no need to
            // ask the model, which may still be waking up
            let perToken = text.count <= Int(Double(Budget.single) * 1.2) ? 1.2 : await charactersPerToken(text)
            let tokens = Int(Double(text.count) / perToken)
            if tokens > Budget.single {
                notes = try await readInParts(text, tokens: tokens, perToken: perToken)
                fromNotes = true
            } else {
                emit("plan", ["parts": 1, "tokens": tokens])
            }
        }
        if fromNotes {
            source = try await fitNotes(notes)
        }

        emit("compose")
        let summary = try await compose(source, fromNotes: fromNotes)
        emit("done", ["data": summary, "notes": notes, "language": language, "seconds": elapsed(since: started)])
    }

    func elapsed(since start: ContinuousClock.Instant) -> Double {
        let time = ContinuousClock.now - start
        return Double(time.components.seconds) + Double(time.components.attoseconds) / 1e18
    }

    /// How many characters one token covers in this text, measured on a few samples
    /// (counting a whole book would take longer than reading a part of it).
    func charactersPerToken(_ text: String) async -> Double {
        if #available(macOS 26.4, *) {
            let size = 3_000
            let count = text.count
            let starts = count <= size * 3 ? [0] : [0, count / 2 - size / 2, count - size]
            var characters = 0
            var tokens = 0
            for offset in starts {
                let from = text.index(text.startIndex, offsetBy: offset)
                let sample = String(text[from...].prefix(size))
                if let n = try? await model.tokenCount(for: sample), n > 0 {
                    characters += sample.count
                    tokens += n
                }
            }
            if tokens > 0 { return max(1.2, Double(characters) / Double(tokens)) }
        }
        return 2.8 // a careful guess: English runs ~3.5, many other languages less
    }

    /// Reads a long text part by part into notes
    func readInParts(_ text: String, tokens: Int, perToken: Double) async throws -> [String] {
        var body = text
        let limit = Int(Double(Budget.part) * perToken * 0.92)
        var kept = 1.0
        if tokens > Budget.part * Budget.parts {
            // A little under the parts' room, as parts end at paragraphs and rarely fill up
            body = condense(text, target: limit * Budget.parts * 9 / 10, windows: Budget.parts)
            kept = Double(body.count) / Double(text.count)
        }
        let parts = split(body, limit: limit)
        emit("plan", ["parts": parts.count, "tokens": tokens, "kept": kept])

        var notes: [String] = []
        for (index, part) in parts.enumerated() {
            emit("part", ["part": index + 1, "parts": parts.count])
            if let note = try await takeNotes(part, part: index + 1, of: parts.count), !note.isEmpty {
                notes.append(note)
            }
        }
        if notes.isEmpty { throw Failure.guardrail }
        return notes
    }

    func notesInstructions() -> String {
        """
        You are reading one part of a longer document and taking notes for a summary of the \
        whole document. Write 3 to 5 bullet points, each starting with "- " and under 25 words, \
        with the most important information in this part: main ideas, decisions, problems, \
        findings, names, amounts, dates and deadlines. Plain text without formatting. Use only \
        what the text says. No introduction, no closing remarks. Write the notes in \(languageName(language)).
        """
    }

    /// Notes for one part, streamed to the app as they are written. A part the model
    /// will not touch (its safety rules) is skipped rather than failing the summary.
    func takeNotes(_ text: String, part: Int, of parts: Int) async throws -> String? {
        let session = LanguageModelSession(model: model, instructions: notesInstructions())
        // `parts` is 0 when notes are shortened again (fitNotes): there is no "of"
        let label = parts > 0 ? "Part \(part) of \(parts)" : "Notes on part \(part)"
        let prompt = "\(meta)\n\(label):\n\n\(text)\n\n\(languageReminder.replacingOccurrences(of: "every field", with: "the notes"))"
        var throttle = Throttle(interval: .milliseconds(120))
        var written = ""
        do {
            let stream = session.streamResponse(
                to: prompt,
                options: GenerationOptions(sampling: .greedy, maximumResponseTokens: 260)
            )
            for try await snapshot in stream {
                written = snapshot.content
                if throttle.ready() { emit("notes", ["part": part, "text": written]) }
            }
        } catch {
            switch classify(error) {
            case .context where text.count > 400:
                // Denser than the samples suggested: read it as two halves
                let half = split(text, limit: text.count / 2 + 1)
                var joined: [String] = []
                for piece in half {
                    if let note = try await takeNotes(piece, part: part, of: parts) { joined.append(note) }
                }
                return joined.joined(separator: "\n")
            case .guardrail, .refusal:
                emit("skipped", ["part": part])
                return nil
            case .language where language != "en":
                language = "en"
                emit("language", ["document": documentLanguage ?? NSNull(), "output": language, "fallback": true])
                return try await takeNotes(text, part: part, of: parts)
            default:
                throw error
            }
        }
        let note = written.trimmingCharacters(in: .whitespacesAndNewlines)
        emit("notes", ["part": part, "text": note, "final": true])
        return note
    }

    /// Joins the notes, and shortens them again when even the notes are too long for the
    /// final summary (a very long document with dense parts).
    func fitNotes(_ notes: [String]) async throws -> String {
        var current = notes
        for _ in 0..<3 {
            let joined = current.enumerated().map { "Part \($0.offset + 1):\n\($0.element)" }.joined(separator: "\n\n")
            let perToken = await charactersPerToken(joined)
            if Double(joined.count) / perToken <= Double(Budget.single) { return joined }
            let limit = Int(Double(Budget.part) * perToken * 0.92)
            var merged: [String] = []
            for group in split(current.joined(separator: "\n"), limit: limit) {
                if let note = try await takeNotes(group, part: merged.count + 1, of: 0) { merged.append(note) }
            }
            if merged.isEmpty || merged.count >= current.count { break }
            current = merged
        }
        // Still too long: keep the start of each part's notes
        let perPart = max(200, Int(Double(Budget.single) * 2.8) / max(current.count, 1))
        return current.enumerated().map { "Part \($0.offset + 1):\n\($0.element.prefix(perPart))" }.joined(separator: "\n\n")
    }

    // MARK: Summary

    func composeInstructions() -> String {
        """
        You are the summary assistant of flipfile, a file app. People drop a document on you \
        because they have no time to read it: tell them what it is about and what matters in it. \
        Use only information from the document; never invent names, numbers or facts. Be \
        specific: concrete details (who, what, how much, when) beat general statements. Write \
        short, plain sentences. Write all text in \(languageName(language)).
        """
    }

    func schema() throws -> GenerationSchema {
        let text = DynamicGenerationSchema(type: String.self)
        let list = { (min: Int, max: Int) in DynamicGenerationSchema(arrayOf: text, minimumElements: min, maximumElements: max) }
        let fact = DynamicGenerationSchema(name: "Fact", properties: [
            .init(name: "label", description: "What the value is, 1 to 4 words", schema: text),
            .init(name: "value", description: "The value exactly as the document gives it: an amount, number, date, deadline or name", schema: text),
        ])
        let summary = DynamicGenerationSchema(name: "Summary", properties: [
            .init(name: "type", description: "What kind of document this is, 1 to 3 words, e.g. Meeting notes, Invoice, Contract, Research paper, Report, Manual, Product guide", schema: text),
            .init(name: "title", description: "A specific headline of at most 9 words that names the subject (the product, project, company, person or event) and what the document says about it", schema: text),
            .init(name: "tldr", description: "The one thing to know: one or two full sentences with the subject and the most important conclusion, decision, result or request", schema: text),
            .init(name: "points", description: "The key points, most important first: what happened, what was decided or found, problems and their consequences, what changes for the reader. One sentence of 8 to 22 words each; do not just repeat the facts", schema: list(1, 6)),
            .init(name: "facts", description: "The figures that matter most: amounts, totals, percentages, quantities, deadlines and key dates. Skip minor details such as meeting rooms or start times", schema: DynamicGenerationSchema(arrayOf: DynamicGenerationSchema(referenceTo: "Fact"), minimumElements: 0, maximumElements: 6)),
            .init(name: "actions", description: "Tasks, requests or next steps someone must do, with who and by when if given. Empty when the document asks nothing of anyone", schema: list(0, 5)),
            .init(name: "keywords", description: "3 to 5 topic keywords, one or two words each", schema: list(2, 5)),
            .init(name: "questions", description: "Up to 3 short questions a reader would want to ask next that the document answers, at most 9 words each", schema: list(0, 3)),
        ])
        return try GenerationSchema(root: summary, dependencies: [fact])
    }

    /// Said last in the prompt too: the model tends to answer in the language it just read
    var languageReminder: String {
        let target = languageName(language)
        if let original = documentLanguage, original != language {
            return "Write every field in \(target), translating from \(languageName(original)); do not answer in \(languageName(original))."
        }
        return "Write every field in \(target)."
    }

    /// The final summary, streamed field by field as the model writes it
    func compose(_ source: String, fromNotes: Bool) async throws -> [String: Any] {
        let prompt = fromNotes
            ? "\(meta)\n\nNotes taken while reading the whole document, part by part:\n\n\(source)\n\n\(languageReminder)"
            : "\(meta)\n\nDocument:\n\n\(source)\n\n\(languageReminder)"
        do {
            return try await composeGuided(prompt)
        } catch {
            switch classify(error) {
            case .guardrail, .refusal, .other:
                // Structured answers use the stricter default safety rules; plain text may
                // transform the user's own content more freely. Same summary, as a template.
                return try await composePlain(prompt)
            case .language where language != "en":
                language = "en"
                emit("language", ["document": documentLanguage ?? NSNull(), "output": language, "fallback": true])
                return try await compose(source, fromNotes: fromNotes)
            default:
                throw error
            }
        }
    }

    /// A finished summary in another language. Asking for a summary "in English" of an
    /// Indonesian text still gets Indonesian from this small model; translating the
    /// summary itself, shown as plain text, works, and is quick (no document to read).
    func translate(_ summary: [String: Any]) async throws -> [String: Any] {
        let from = documentLanguage.map(languageName) ?? "its language"
        let to = languageName(language)
        let instructions = """
        You translate document summaries from \(from) into \(to). Translate every text \
        faithfully and completely into natural \(to); keep names, numbers, amounts and dates \
        exactly as they are, and keep every list item. Never leave text in \(from).
        """
        let prompt = "Summary to translate into \(to):\n\n\(readable(summary))\n\nNow write this summary in \(to)."
        do {
            return try await composeGuided(prompt, instructions: instructions)
        } catch {
            switch classify(error) {
            case .guardrail, .refusal, .other: return try await composePlain(prompt, instructions: instructions)
            default: throw error
            }
        }
    }

    /// A summary as labelled plain text, the way a person would write it
    func readable(_ summary: [String: Any]) -> String {
        var lines: [String] = []
        let text = { (key: String) in (summary[key] as? String)?.trimmingCharacters(in: .whitespaces) ?? "" }
        let list = { (key: String) in (summary[key] as? [Any])?.compactMap { $0 as? String } ?? [] }
        for (label, key) in [("Type", "type"), ("Title", "title"), ("Summary", "tldr")] where !text(key).isEmpty {
            lines.append("\(label): \(text(key))")
        }
        let facts = (summary["facts"] as? [[String: Any]] ?? []).map { "- \($0["label"] as? String ?? ""): \($0["value"] as? String ?? "")" }
        for (label, items) in [("Key points", list("points").map { "- \($0)" }), ("Figures", facts), ("To do", list("actions").map { "- \($0)" })] where !items.isEmpty {
            lines.append("\(label):\n" + items.joined(separator: "\n"))
        }
        if !list("keywords").isEmpty { lines.append("Keywords: " + list("keywords").joined(separator: ", ")) }
        if !list("questions").isEmpty { lines.append("Questions:\n" + list("questions").map { "- \($0)" }.joined(separator: "\n")) }
        return lines.joined(separator: "\n")
    }

    // MARK: Questions

    /// Answers a question about the document from the passages that share the most
    /// (telling) words with it, after the summary as an overview: the whole document
    /// rarely fits the model at once.
    func answer(_ question: String) async throws {
        let text = request.text
        let perToken = await charactersPerToken(text)
        var overview = ""
        if let json = request.summary, let data = json.data(using: .utf8),
           let summary = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] {
            overview = readable(summary)
        }
        let room = Int(Double(Budget.single) * perToken) - overview.count
        var excerpts: [String]
        if text.count <= room {
            excerpts = [text]
        } else {
            let pieces = passages(text, limit: 900)
            var picked: [Int] = []
            var used = 0
            for index in rank(pieces, by: question) where used + pieces[index].count <= room {
                picked.append(index)
                used += pieces[index].count + 8
            }
            // Best match first: a small model leans on what it reads first
            excerpts = picked.map { pieces[$0] }
        }
        if !overview.isEmpty { excerpts.insert("Overview (a summary of the whole document):\n\(overview)", at: 0) }

        // The question's own language, when it is clear enough to tell
        let recognizer = NLLanguageRecognizer()
        recognizer.processString(question)
        if let (asked, confidence) = recognizer.languageHypotheses(withMaximum: 1).first, confidence > 0.6 {
            language = asked.rawValue
        }
        let instructions = """
        You answer questions about a document using only the excerpts given. Answer directly \
        in one to four short sentences, with the concrete details (names, amounts, dates); \
        when the excerpts answer only part of the question, give that part. Only when they \
        say nothing about it, say briefly that the document does not say. Never invent facts. \
        Answer in \(languageName(language)).
        """
        let session = LanguageModelSession(model: model, instructions: instructions)
        let prompt = "Question: \(question)\n\nExcerpts from \"\(request.name ?? "the document")\":\n\n"
            + excerpts.joined(separator: "\n\n---\n\n")
            + "\n\nQuestion again: \(question)\nAnswer in \(languageName(language))."
        var throttle = Throttle(interval: .milliseconds(60))
        var written = ""
        let stream = session.streamResponse(to: prompt, options: GenerationOptions(sampling: .greedy, maximumResponseTokens: 300))
        for try await snapshot in stream {
            written = snapshot.content
            if throttle.ready() { emit("answer", ["text": written]) }
        }
        emit("answered", ["text": written.trimmingCharacters(in: .whitespacesAndNewlines), "language": language])
    }

    /// Indices of `pieces`, best match for `question` first (BM25). Words are cut to
    /// their first six letters, so "upload" finds "uploads" and "convert" "conversion";
    /// figures stay whole ("0.5.2", "3,2").
    func rank(_ pieces: [String], by question: String) -> [Int] {
        let words = { (s: String) in
            s.lowercased()
                .split(whereSeparator: { !$0.isLetter && !$0.isNumber && $0 != "." && $0 != "," })
                .map { $0.trimmingCharacters(in: CharacterSet(charactersIn: ".,")) }
                .filter { $0.count >= 3 || $0.contains(where: \.isNumber) }
                .map { String($0.prefix(6)) }
        }
        let asked = Set(words(question))
        let terms = pieces.map(words)
        let average = Double(terms.map(\.count).reduce(0, +)) / Double(max(pieces.count, 1))
        var spread: [String: Int] = [:]
        for piece in terms {
            for word in Set(piece) where asked.contains(word) { spread[word, default: 0] += 1 }
        }
        let scores = terms.map { piece -> Double in
            var counts: [String: Int] = [:]
            for word in piece where asked.contains(word) { counts[word, default: 0] += 1 }
            return counts.reduce(0.0) { sum, entry in
                var idf = log(1 + (Double(pieces.count) - Double(spread[entry.key] ?? 0) + 0.5) / (Double(spread[entry.key] ?? 0) + 0.5))
                // A figure asked for (a version, a date, an amount) pins the passage down
                if entry.key.contains(where: \.isNumber) { idf *= 2 }
                let tf = Double(entry.value)
                return sum + idf * tf * 2.2 / (tf + 1.2 * (0.25 + 0.75 * Double(piece.count) / max(average, 1)))
            }
        }
        return pieces.indices.sorted { scores[$0] > scores[$1] }
    }

    func composeGuided(_ prompt: String, instructions: String? = nil) async throws -> [String: Any] {
        let session = LanguageModelSession(model: model, instructions: instructions ?? composeInstructions())
        let stream = session.streamResponse(
            to: prompt,
            schema: try schema(),
            options: GenerationOptions(sampling: .greedy, maximumResponseTokens: 900)
        )
        var throttle = Throttle(interval: .milliseconds(60))
        var latest: [String: Any] = [:]
        for try await snapshot in stream {
            guard let data = snapshot.rawContent.jsonString.data(using: .utf8),
                  let object = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
            else { continue }
            latest = object
            if throttle.ready() { emit("summary", ["data": object]) }
        }
        return latest
    }

    func composePlain(_ prompt: String, instructions base: String? = nil) async throws -> [String: Any] {
        let instructions = (base ?? composeInstructions()) + """
         Answer in exactly this format, with these English labels:
        TYPE: what kind of document this is, 1 to 3 words
        TITLE: a short, specific headline
        TLDR: the one thing to know, one or two sentences
        POINTS:
        - a key point
        FACTS:
        - label: value
        ACTIONS:
        - a task or next step (leave empty if there are none)
        KEYWORDS: keyword, keyword, keyword
        """
        let session = LanguageModelSession(model: model, instructions: instructions)
        let stream = session.streamResponse(to: prompt, options: GenerationOptions(sampling: .greedy, maximumResponseTokens: 900))
        var throttle = Throttle(interval: .milliseconds(60))
        var written = ""
        for try await snapshot in stream {
            written = snapshot.content
            if throttle.ready() { emit("summary", ["data": parseTemplate(written)]) }
        }
        return parseTemplate(written)
    }
}

/// Reads the plain-text template of `composePlain` into the same fields as the schema
func parseTemplate(_ text: String) -> [String: Any] {
    var single: [String: String] = [:]
    var lists: [String: [String]] = ["points": [], "facts": [], "actions": []]
    var section = ""
    let labels = ["TYPE": "type", "TITLE": "title", "TLDR": "tldr", "TL;DR": "tldr", "POINTS": "points",
                  "FACTS": "facts", "ACTIONS": "actions", "KEYWORDS": "keywords"]
    for raw in text.split(separator: "\n") {
        let line = raw.trimmingCharacters(in: .whitespaces)
        if let colon = line.firstIndex(of: ":"),
           let key = labels[line[..<colon].trimmingCharacters(in: CharacterSet(charactersIn: "*# ")).uppercased()] {
            let value = line[line.index(after: colon)...].trimmingCharacters(in: .whitespaces)
            section = key
            if key == "keywords" {
                lists["keywords"] = value.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
            } else if lists[key] == nil, !value.isEmpty {
                single[key] = value
            }
            continue
        }
        guard line.hasPrefix("-") || line.hasPrefix("•") || line.hasPrefix("*") else { continue }
        let item = line.dropFirst().trimmingCharacters(in: .whitespaces)
        guard !item.isEmpty, lists[section] != nil, section != "keywords" else { continue }
        lists[section]?.append(item)
    }
    var out: [String: Any] = single
    for (key, items) in lists where !items.isEmpty || key == "actions" {
        if key == "facts" {
            out[key] = items.map { item -> [String: String] in
                guard let colon = item.firstIndex(of: ":") else { return ["label": "", "value": item] }
                return ["label": item[..<colon].trimmingCharacters(in: .whitespaces),
                        "value": item[item.index(after: colon)...].trimmingCharacters(in: .whitespaces)]
            }
        } else {
            out[key] = items
        }
    }
    return out
}

// MARK: - Entry

@main
struct FlipfileAI {
    static func main() async {
        switch CommandLine.arguments.dropFirst().first {
        case "status":
            if let reason = unavailableReason() {
                emit("status", ["available": false, "reason": reason])
            } else {
                emit("status", ["available": true])
            }
        case "summarize":
            await summarize()
        case "warm":
            await warm()
        default:
            FileHandle.standardError.write(Data("usage: flipfile-ai status | warm | summarize < request.json\n".utf8))
            exit(2)
        }
    }

    /// Wakes the model up before it is needed: after a while unused it takes seconds to
    /// load again, which the app overlaps with the drag and with reading the file. The
    /// model runs in a system service, so it stays loaded after this process ends.
    static func warm() async {
        guard unavailableReason() == nil else { return }
        let model = SystemLanguageModel(useCase: .general, guardrails: .permissiveContentTransformations)
        if #available(macOS 26.4, *) {
            // Returns once the model has answered, so loaded
            _ = try? await model.tokenCount(for: "flipfile")
        } else {
            LanguageModelSession(model: model).prewarm()
            try? await Task.sleep(for: .seconds(4))
        }
        emit("warm")
    }

    static func summarize() async {
        let input = FileHandle.standardInput.readDataToEndOfFile()
        guard let request = try? JSONDecoder().decode(Request.self, from: input) else {
            emit("error", ["code": Failure.request.rawValue, "message": "The request could not be read"])
            return
        }
        if let reason = unavailableReason() {
            emit("error", ["code": reason, "message": "Apple Intelligence is not available (\(reason))"])
            return
        }
        do {
            try await Summarizer(request).run()
        } catch {
            emit("error", ["code": classify(error).rawValue, "message": String(describing: error)])
        }
    }
}
