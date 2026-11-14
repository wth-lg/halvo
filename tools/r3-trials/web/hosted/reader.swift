// The policy-page reader of U0.7c's ninth run.
//   reader pdf PDF NAME VALUE [TEXT_OUT]   reads the PDF's text with PDFKit (and writes it to TEXT_OUT)
//   reader text FILE NAME VALUE            reads a text file, for tries without a browser
// It prints booleans, counts and closed-list words only:
//   policy_page  the text holds "policy name" in any case, with spaces and line ends taken out
//   named        yes when the name is in the text (named_by=text), in the text squashed to letters and
//                digits (named_by=squashed: line ends, spaces, soft hyphens and zero-width marks
//                gone), or in pieces: a word that ends with a prefix of the name (6 letters, or half
//                of a short name and at least 4), then words up to 12 lines after or before it that
//                carry the rest, other words between them skipped (named_by=pieces); else no
//                (named_by=none)
//   prefix_len   the longest prefix of the name that the squashed text holds, of name_len
//   near         the words of the name's first line and the six lines after it, as false, direct,
//                Platform, Machine, Mandatory, OK or other (a piece of the name itself is skipped)
//   value_seen   near holds VALUE
//   platform_count  how often the text holds "Platform"
import Foundation
import PDFKit

func squash(_ s: String) -> String {
    String(String.UnicodeScalarView(s.unicodeScalars.filter { CharacterSet.alphanumerics.contains($0) }))
}

let a = CommandLine.arguments
guard a.count >= 5, a[1] == "pdf" || a[1] == "text" else {
    print("policy_page=no readable=no named=no named_by=none prefix_len=0 name_len=0 value_seen=no platform_count=0 near=-")
    exit(2)
}
let name = a[3]
let value = a[4]
var text = ""
if a[1] == "pdf" {
    guard let doc = PDFDocument(url: URL(fileURLWithPath: a[2])) else {
        print("policy_page=no readable=no named=no named_by=none prefix_len=0 name_len=\(name.count) value_seen=no platform_count=0 near=-")
        exit(2)
    }
    text = doc.string ?? ""
    if a.count >= 6 { try? text.write(toFile: a[5], atomically: true, encoding: .utf8) }
} else {
    text = (try? String(contentsOfFile: a[2], encoding: .utf8)) ?? ""
}
let lines = text.components(separatedBy: .newlines)
let sq = lines.map(squash)
let all = squash(text)

// the line where the name starts and how it was found
var start: Int? = nil
var by = "none"
if let i = lines.firstIndex(where: { $0.contains(name) }) {
    start = i; by = "text"
} else if all.contains(name) {
    by = "squashed"
    func window(_ i: Int) -> String { sq[i..<min(sq.count, i + 13)].joined() }
    if var i = sq.indices.first(where: { window($0).contains(name) }) {
        let first = i
        while i + 1 < sq.count && i + 1 <= first + 12 && window(i + 1).contains(name) { i += 1 }
        start = i
    }
} else {
    // pieces: a word that ends with a prefix of the name (6 characters or more), then words on
    // the lines up to 12 before or after it that carry the rest piece by piece, other words skipped
    var words: [(line: Int, w: String)] = []
    for (n, l) in lines.enumerated() {
        for w in l.split(whereSeparator: { $0.isWhitespace }) {
            let q = squash(String(w))
            if !q.isEmpty { words.append((n, q)) }
        }
    }
    // the shortest first piece: 6 letters, or half the name for a short name (at least 4)
    let least = min(6, max(4, name.count / 2))
    outer: for (wi, x) in words.enumerated() {
        var p = min(x.w.count, name.count - 1)
        while p >= least && !x.w.hasSuffix(String(name.prefix(p))) { p -= 1 }
        if p < least { continue }
        let near = words.indices.filter { $0 != wi && abs(words[$0].line - x.line) <= 12 }
        let order = near.filter { $0 > wi } + near.filter { $0 < wi }.reversed()
        var pos = p
        for k in order {
            let rest = String(name.dropFirst(pos))
            let t = words[k].w
            if t.hasPrefix(rest) { start = x.line; by = "pieces"; break outer }
            if rest.hasPrefix(t) && t.count >= 2 { pos += t.count }
        }
    }
}
var prefix = 0
while prefix < name.count && all.contains(String(name.prefix(prefix + 1))) { prefix += 1 }

let known: Set<String> = ["false", "direct", "Platform", "Machine", "Mandatory", "OK"]
var near: [String] = []
if let i = start {
    for l in lines[i..<min(lines.count, i + 7)] {
        for w in l.split(whereSeparator: { $0 == " " || $0 == "\t" }).map(String.init) {
            let q = squash(w)
            if q.count >= 2 && name.contains(q) { continue }
            let t = known.contains(w) ? w : "other"
            if !(t == "other" && near.last == "other") { near.append(t) }
        }
    }
}
let page = squash(text.lowercased()).contains("policyname")
let platform = text.components(separatedBy: "Platform").count - 1
print("policy_page=\(page ? "yes" : "no") readable=yes named=\(by == "none" ? "no" : "yes") named_by=\(by) prefix_len=\(prefix) name_len=\(name.count) value_seen=\(near.contains(value) ? "yes" : "no") platform_count=\(platform) near=\(near.isEmpty ? "-" : near.joined(separator: ","))")
