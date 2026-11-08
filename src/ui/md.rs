//! Markdown + math for AI replies. LaTeX is converted to readable Unicode (a ∣ b, ∀x ∈ ℤ, n²),
//! which the bundled Noto Sans Math font can draw, so no web/math engine is needed.

use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use regex::Regex;
use std::sync::LazyLock;

const SYMBOLS: &[(&str, &str)] = &[
    ("\\forall", "∀"), ("\\exists", "∃"), ("\\nexists", "∄"), ("\\neg", "¬"), ("\\lnot", "¬"), ("\\land", "∧"), ("\\wedge", "∧"), ("\\lor", "∨"),
    ("\\vee", "∨"), ("\\oplus", "⊕"), ("\\implies", "⇒"), ("\\Rightarrow", "⇒"), ("\\Leftarrow", "⇐"), ("\\iff", "⇔"), ("\\Leftrightarrow", "⇔"),
    ("\\rightarrow", "→"), ("\\to", "→"), ("\\leftarrow", "←"), ("\\gets", "←"), ("\\leftrightarrow", "↔"), ("\\mapsto", "↦"), ("\\longrightarrow", "⟶"),
    ("\\notin", "∉"), ("\\in", "∈"), ("\\ni", "∋"), ("\\subseteq", "⊆"), ("\\subset", "⊂"), ("\\supseteq", "⊇"), ("\\supset", "⊃"), ("\\nsubseteq", "⊈"),
    ("\\cup", "∪"), ("\\cap", "∩"), ("\\bigcup", "⋃"), ("\\bigcap", "⋂"), ("\\setminus", "∖"), ("\\emptyset", "∅"), ("\\varnothing", "∅"),
    ("\\nmid", "∤"), ("\\mid", "∣"), ("\\leq", "≤"), ("\\le", "≤"), ("\\geq", "≥"), ("\\ge", "≥"), ("\\neq", "≠"), ("\\ne", "≠"),
    ("\\approx", "≈"), ("\\equiv", "≡"), ("\\cong", "≅"), ("\\sim", "∼"), ("\\propto", "∝"), ("\\pm", "±"), ("\\mp", "∓"), ("\\times", "×"),
    ("\\cdot", "·"), ("\\cdots", "⋯"), ("\\ldots", "…"), ("\\dots", "…"), ("\\vdots", "⋮"), ("\\div", "÷"), ("\\circ", "∘"), ("\\infty", "∞"),
    ("\\sum", "Σ"), ("\\prod", "Π"), ("\\int", "∫"), ("\\partial", "∂"), ("\\nabla", "∇"), ("\\therefore", "∴"), ("\\because", "∵"),
    ("\\lfloor", "⌊"), ("\\rfloor", "⌋"), ("\\lceil", "⌈"), ("\\rceil", "⌉"), ("\\langle", "⟨"), ("\\rangle", "⟩"), ("\\|", "‖"),
    ("\\alpha", "α"), ("\\beta", "β"), ("\\gamma", "γ"), ("\\delta", "δ"), ("\\epsilon", "ε"), ("\\varepsilon", "ε"), ("\\zeta", "ζ"), ("\\eta", "η"),
    ("\\theta", "θ"), ("\\iota", "ι"), ("\\kappa", "κ"), ("\\lambda", "λ"), ("\\mu", "μ"), ("\\nu", "ν"), ("\\xi", "ξ"), ("\\pi", "π"), ("\\rho", "ρ"),
    ("\\sigma", "σ"), ("\\tau", "τ"), ("\\phi", "φ"), ("\\varphi", "φ"), ("\\chi", "χ"), ("\\psi", "ψ"), ("\\omega", "ω"),
    ("\\Gamma", "Γ"), ("\\Delta", "Δ"), ("\\Theta", "Θ"), ("\\Lambda", "Λ"), ("\\Sigma", "Σ"), ("\\Phi", "Φ"), ("\\Psi", "Ψ"), ("\\Omega", "Ω"), ("\\Pi", "Π"),
    ("\\top", "⊤"), ("\\bot", "⊥"), ("\\vdash", "⊢"), ("\\models", "⊨"), ("\\square", "□"), ("\\blacksquare", "■"), ("\\qed", "∎"), ("\\checkmark", "✓"),
    ("\\deg", "deg"), ("\\log", "log"), ("\\ln", "ln"), ("\\sin", "sin"), ("\\cos", "cos"), ("\\tan", "tan"), ("\\max", "max"), ("\\min", "min"),
    ("\\gcd", "gcd"), ("\\lim", "lim"), ("\\bmod", "mod"), ("\\mod", "mod"), ("\\quad", "  "), ("\\qquad", "    "), ("\\,", " "), ("\\;", " "),
    ("\\:", " "), ("\\!", ""), ("\\left", ""), ("\\right", ""), ("\\big", ""), ("\\Big", ""), ("\\displaystyle", ""), ("\\{", "{"), ("\\}", "}"),
    ("\\%", "%"), ("\\&", "&"), ("\\#", "#"), ("\\_", "_"), ("\\ ", " "),
];

fn sup(c: char) -> Option<char> {
    Some(match c {
        '0' => '⁰', '1' => '¹', '2' => '²', '3' => '³', '4' => '⁴', '5' => '⁵', '6' => '⁶', '7' => '⁷', '8' => '⁸', '9' => '⁹',
        '+' => '⁺', '-' => '⁻', '=' => '⁼', '(' => '⁽', ')' => '⁾', 'n' => 'ⁿ', 'i' => 'ⁱ', 'k' => 'ᵏ', 'm' => 'ᵐ', 'x' => 'ˣ', 'a' => 'ᵃ',
        'b' => 'ᵇ', 'c' => 'ᶜ', 'd' => 'ᵈ', 'e' => 'ᵉ', 'j' => 'ʲ', 'r' => 'ʳ', 't' => 'ᵗ', 'y' => 'ʸ', 'T' => 'ᵀ', '*' => '*', ' ' => ' ',
        _ => return None,
    })
}

fn sub(c: char) -> Option<char> {
    Some(match c {
        '0' => '₀', '1' => '₁', '2' => '₂', '3' => '₃', '4' => '₄', '5' => '₅', '6' => '₆', '7' => '₇', '8' => '₈', '9' => '₉',
        '+' => '₊', '-' => '₋', '=' => '₌', '(' => '₍', ')' => '₎', 'n' => 'ₙ', 'i' => 'ᵢ', 'k' => 'ₖ', 'm' => 'ₘ', 'x' => 'ₓ', 'a' => 'ₐ',
        'e' => 'ₑ', 'j' => 'ⱼ', 'r' => 'ᵣ', 't' => 'ₜ', 'o' => 'ₒ', 'p' => 'ₚ', 's' => 'ₛ', 'h' => 'ₕ', 'l' => 'ₗ', ' ' => ' ',
        _ => return None,
    })
}

fn blackboard(c: char) -> char {
    match c {
        'N' => 'ℕ', 'Z' => 'ℤ', 'Q' => 'ℚ', 'R' => 'ℝ', 'C' => 'ℂ', 'P' => 'ℙ',
        _ => c,
    }
}

/// Convert one LaTeX math expression to Unicode text.
pub fn latex_to_unicode(src: &str) -> String {
    static GROUPS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\(?:text|mathrm|textbf|mathbf|mathit|operatorname|textit|mbox)\{([^{}]*)\}").unwrap());
    static BB: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\mathbb\{([A-Za-z])\}").unwrap());
    static FRAC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\[dt]?frac\{([^{}]*)\}\{([^{}]*)\}").unwrap());
    static SQRT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\sqrt\{([^{}]*)\}").unwrap());
    static BINOM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\binom\{([^{}]*)\}\{([^{}]*)\}").unwrap());
    static OVER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\(?:overline|bar)\{([^{}]*)\}").unwrap());
    static SCRIPT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([\^_])(\{([^{}]*)\}|(\\[A-Za-z]+|.))").unwrap());
    static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\[A-Za-z]+|\\[|,;:! %&#_{}]").unwrap());
    let mut s = src.to_string();
    for _ in 0..3 {
        s = GROUPS.replace_all(&s, "$1").to_string();
        s = BB.replace_all(&s, |c: &regex::Captures| blackboard(c[1].chars().next().unwrap()).to_string()).to_string();
        s = FRAC.replace_all(&s, |c: &regex::Captures| {
            let (a, b) = (c[1].trim(), c[2].trim());
            match (a, b) {
                ("1", "2") => "½".into(), ("1", "3") => "⅓".into(), ("1", "4") => "¼".into(), ("3", "4") => "¾".into(),
                _ => {
                    let wrap = |x: &str| if x.chars().count() > 1 && x.contains(|ch: char| "+-·× ".contains(ch)) { format!("({x})") } else { x.to_string() };
                    format!("{}/{}", wrap(a), wrap(b))
                }
            }
        }).to_string();
        s = SQRT.replace_all(&s, |c: &regex::Captures| if c[1].chars().count() == 1 { format!("√{}", &c[1]) } else { format!("√({})", &c[1]) }).to_string();
        s = BINOM.replace_all(&s, "C($1, $2)").to_string();
        s = OVER.replace_all(&s, |c: &regex::Captures| c[1].chars().flat_map(|ch| [ch, '\u{0305}']).collect::<String>()).to_string();
    }
    s = WORD.replace_all(&s, |c: &regex::Captures| {
        let w = &c[0];
        SYMBOLS.iter().find(|(k, _)| *k == w).map(|(_, v)| v.to_string()).unwrap_or_else(|| w.trim_start_matches('\\').to_string())
    }).to_string();
    s = SCRIPT.replace_all(&s, |c: &regex::Captures| {
        let body = c.get(3).map(|m| m.as_str()).or(c.get(4).map(|m| m.as_str())).unwrap_or("");
        let f = if &c[1] == "^" { sup } else { sub };
        let conv: Option<String> = body.chars().map(f).collect();
        match conv {
            Some(t) => t,
            None => format!("{}({body})", &c[1]),
        }
    }).to_string();
    s.replace(['{', '}'], "")
}

/// Replace $...$, $$...$$, \(...\), \[...\] with Unicode. Display math becomes its own centered line.
pub fn math_to_unicode(text: &str) -> String {
    static DISPLAY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)\$\$(.+?)\$\$|\\\[(.+?)\\\]").unwrap());
    static INLINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\\((.+?)\\\)|\$([^\s$](?:[^$\n]*?[^\s$])?)\$").unwrap());
    let t = DISPLAY.replace_all(text, |c: &regex::Captures| {
        let m = c.get(1).or(c.get(2)).map(|m| m.as_str()).unwrap_or("");
        format!("\n\n**{}**\n\n", latex_to_unicode(m.trim()).replace('*', "∗"))
    });
    INLINE.replace_all(&t, |c: &regex::Captures| {
        let m = c.get(1).or(c.get(2)).map(|m| m.as_str()).unwrap_or("");
        latex_to_unicode(m).replace('*', "∗")
    }).to_string()
}

pub enum Segment {
    Md(String),
    Check(serde_json::Value),
    Pending, // a check card still being written
}

/// Split an AI reply into Markdown and interactive check-question cards.
pub fn segments(text: &str, live: bool) -> Vec<Segment> {
    static CHECK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)```check\s*\n?(.*?)```").unwrap());
    let mut out = vec![];
    let mut last = 0;
    for c in CHECK.captures_iter(text) {
        let m = c.get(0).unwrap();
        out.push(Segment::Md(text[last..m.start()].to_string()));
        match serde_json::from_str::<serde_json::Value>(c[1].trim()) {
            Ok(v) if v["options"].is_array() => out.push(Segment::Check(v)),
            _ => out.push(Segment::Md(m.as_str().to_string())),
        }
        last = m.end();
    }
    let rest = &text[last..];
    if live {
        if let Some(i) = rest.find("```check") {
            out.push(Segment::Md(rest[..i].to_string()));
            out.push(Segment::Pending);
            return out;
        }
    }
    out.push(Segment::Md(rest.to_string()));
    out
}

pub fn show(ui: &mut egui::Ui, cache: &mut CommonMarkCache, text: &str) {
    let t = math_to_unicode(text);
    CommonMarkViewer::new().show(ui, cache, &t);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn math() {
        assert_eq!(latex_to_unicode(r"\forall x \in \mathbb{Z}"), "∀ x ∈ ℤ");
        assert_eq!(latex_to_unicode("n^2 + a_1"), "n² + a₁");
        assert_eq!(latex_to_unicode(r"a \mid b"), "a ∣ b");
        assert_eq!(latex_to_unicode(r"\frac{1}{2}"), "½");
        assert_eq!(latex_to_unicode(r"2^{n+1}"), "2ⁿ⁺¹");
        assert_eq!(math_to_unicode("so $E=mc^2$ ok"), "so E=mc² ok");
    }
}
