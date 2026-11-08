//! Lumi's AI jobs: prompts, study-material context and lesson storage.
//! Which AI runs each job is decided in llm.rs.

use crate::db::Db;
use crate::game;
use crate::llm::{self, Ev, LlmError, Msg, Part, Pdf, Provider};
use crate::materials;
use crate::model::{Concept, Course, Material, Session, Test};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

pub const SYSTEM: &str = r#"You are Lumi, a warm, sharp, endlessly patient personal tutor inside a learning app. Your job is to take the learner from zero to genuine mastery of their course, using the study materials they uploaded (textbooks, notes, quizzes, problem sets, slides, images) as the primary reference.

How you teach:
- Ground explanations in the learner's materials. When a concept, definition, example or problem comes from them, say where (e.g. "Section 4.7, p. 227" or the file name) so they can follow along. For long textbooks you see the table of contents plus the pages relevant to the current topic. If you need a part you weren't given, name the section and the app will pull it up next turn. If the materials don't cover something, say so and teach it anyway from solid general knowledge.
- Diagnose before lecturing: early in a lesson, find out what the learner already knows with a quick question or two, then pitch the explanation at their level.
- Teach in small steps: intuition first, then the precise idea, then a worked example, then let the learner try. Prefer concrete examples, analogies and visuals-in-words over walls of text. Keep each message focused; a lesson is a conversation, not a lecture.
- Use retrieval practice constantly. After explaining something, check understanding before moving on. Questions that make the learner produce an answer beat re-reading.
- Feedback is immediate and specific: when they're wrong, find the exact misconception, explain why it's tempting, and give a fresh question to try again. When they're right, say what was good about it and raise the difficulty a little.
- For problems from their materials, coach rather than solve outright: ask what they've tried, give the next hint, and only show a full solution when they're stuck or ask for it - then have them do a similar problem themselves.
- The learner can send photos: homework problems, textbook pages, whiteboards, their handwritten work. Read them carefully. For their own work, find the exact step where it goes wrong and say why, rather than just giving the right answer.
- Be encouraging and genuine, never saccharine. Celebrate progress briefly and specifically. Normalize mistakes as part of learning.
- Use Markdown. Use LaTeX for math: $...$ inline and $$...$$ for display.

Interactive check questions: the app can render a tappable multiple-choice card. To use one, put exactly this in your message (a fenced block with the language tag `check` containing one JSON object):
```check
{"question": "...", "options": ["...", "...", "...", "..."], "answer": 0, "explanation": "why the right answer is right and the tempting wrong one is wrong"}
```
"answer" is the 0-based index of the correct option. Don't reveal the answer in the surrounding text. Use at most one check card per message, end the message after it, and wait for the learner's result (the app sends it to you). Mix these with open questions where the learner has to explain or calculate in their own words."#;

pub type Exclude = HashMap<i64, HashSet<u32>>;

// ---------- study materials -> AI context ----------

/// Small materials whole; big ones as table of contents + the pages relevant to `query`.
/// Returns the parts and the excerpt references used: [(material_id, pages)].
pub fn material_parts(db: &Db, ids: &[i64], query: Option<&str>, max_pages: usize, include_small: bool, exclude: &Exclude) -> (Vec<Part>, Vec<(i64, Vec<u32>)>) {
    let mut parts = vec![];
    let mut refs = vec![];
    for &mid in ids {
        let Some(mut m) = Material::get(db, mid) else { continue };
        if m.kind != "image" && !m.indexed {
            materials::ensure_ready(db, mid);
            m = match Material::get(db, mid) { Some(x) => x, None => continue };
        }
        if materials::is_large(&m) {
            materials::ensure_ready(db, mid);
            if include_small {
                let toc = materials::toc_text(&m);
                parts.push(Part::Text {
                    aux: true, // dropped for AIs with tight limits
                    text: format!("Table of contents of \"{}\" ({} pages; you get relevant pages, not the whole book):\n{}", m.filename, m.pages,
                        if toc.is_empty() { "(no outline in this file)".into() } else { toc }),
                });
            }
            if let Some(q) = query {
                let pages = materials::retrieve(db, &m, q, max_pages, exclude.get(&mid).unwrap_or(&HashSet::new()));
                if !pages.is_empty() {
                    parts.push(excerpt_part(db, &m, &pages));
                    refs.push((mid, pages));
                }
            }
        } else if include_small {
            match m.kind.as_str() {
                "pdf" => parts.push(Part::Doc { title: m.filename.clone(), text: materials::whole_text(db, &m), pdf: m.path.as_ref().map(|p| Pdf::File(PathBuf::from(p))) }),
                "image" => {
                    if let Some(data) = m.path.as_ref().and_then(|p| std::fs::read(p).ok()) {
                        parts.push(Part::text(format!("(Study material image: {})", m.filename)));
                        parts.push(Part::Image { data, mime: m.mime.clone() });
                    }
                }
                _ => parts.push(Part::Doc { title: m.filename.clone(), text: materials::whole_text(db, &m), pdf: None }),
            }
        }
    }
    (parts, refs)
}

pub fn excerpt_part(db: &Db, m: &Material, pages: &[u32]) -> Part {
    let title = format!("{}: {}", m.filename, materials::label_pages(m, pages));
    let pdf = if m.kind == "pdf" { materials::excerpt_pdf(db, m, pages).map(Pdf::Bytes) } else { None };
    Part::Doc { title, text: materials::page_texts(db, m, pages), pdf }
}

pub fn course_header(db: &Db, course: &Course) -> String {
    let mut lines = vec![format!("Course: {}", course.name)];
    if !course.goal.is_empty() {
        lines.push(format!("Learner's goal: {}", course.goal));
    }
    if let Some(n) = db.get::<String>("name").filter(|n| !n.is_empty()) {
        lines.push(format!("Learner's name: {n}"));
    }
    let concepts = Concept::for_course(db, course.id);
    if !concepts.is_empty() {
        lines.push("Learning path (mastery 0-100):".into());
        let mut unit = String::new();
        for c in concepts {
            if c.unit_title != unit {
                unit = c.unit_title.clone();
                lines.push(format!("  Unit: {unit}"));
            }
            lines.push(format!("    - {} (mastery {})", c.title, c.mastery as i64));
        }
    }
    lines.join("\n")
}

fn json_job(db: &Db, job: &str, mut parts: Vec<Part>, instruction: String, schema: &Value, effort: &str) -> Result<(Value, Provider), LlmError> {
    parts.push(Part::text(instruction));
    llm::json_call(db, job, SYSTEM, &[Msg::user(parts)], schema, 16000, effort)
}

// ---------- learning path ----------

pub fn curriculum_schema() -> Value {
    json!({"type": "object", "properties": {
        "emoji": {"type": "string"},
        "units": {"type": "array", "items": {"type": "object", "properties": {
            "title": {"type": "string"},
            "concepts": {"type": "array", "items": {"type": "object", "properties": {"title": {"type": "string"}, "summary": {"type": "string"}},
                "required": ["title", "summary"], "additionalProperties": false}}},
            "required": ["title", "concepts"], "additionalProperties": false}}},
        "required": ["emoji", "units"], "additionalProperties": false})
}

pub fn generate_curriculum(db: &Db, course: &Course) -> Result<Value, LlmError> {
    let (parts, _) = material_parts(db, &Material::enabled_ids(db, course.id), None, 0, true, &Exclude::new());
    let instruction = format!("{}\n\nDesign a learning path that takes this learner from no prior knowledge to mastery of this course, based on the study materials above (for long textbooks, use the table of contents; quizzes, notes and syllabi show what the course actually emphasizes). If materials are thin, use the course name and goal.
- Order units and concepts so each builds on the previous ones; start with foundations.
- Usually 3-8 units with 3-6 concepts each; scale to the material. Each concept should be one teachable, testable idea (one lesson + practice session).
- Concept titles: short (2-6 words). Summaries: 1-2 sentences naming what the learner will be able to do, and where it's covered (e.g. \"Section 4.7\") if applicable.
- \"emoji\": one emoji that fits the subject.", course_header(db, course));
    Ok(json_job(db, "read", parts, instruction, &curriculum_schema(), "high")?.0)
}

/// Replace a course's learning path, keeping progress on concepts that survive a regenerate.
pub fn save_curriculum(db: &Db, course: &Course, data: &Value) {
    let old: HashMap<String, Concept> = Concept::for_course(db, course.id).into_iter().filter(|c| c.unit_idx < 1000).map(|c| (c.title.to_lowercase(), c)).collect();
    db.exec("DELETE FROM concepts WHERE course_id=? AND unit_idx<1000", &[&course.id]);
    for (ui, unit) in data["units"].as_array().into_iter().flatten().enumerate() {
        let ut = unit["title"].as_str().unwrap_or("Unit");
        for (ci, c) in unit["concepts"].as_array().into_iter().flatten().enumerate() {
            let title = c["title"].as_str().unwrap_or("Concept");
            let summary = c["summary"].as_str().unwrap_or("");
            let prev = old.get(&title.to_lowercase());
            let (mastery, bx, due, att) = prev.map(|p| (p.mastery, p.bx, p.due_at.clone(), p.attempts)).unwrap_or((0.0, 0, None, 0));
            let (ui, ci) = (ui as i64, ci as i64);
            db.exec("INSERT INTO concepts(course_id, unit_idx, unit_title, idx, title, summary, mastery, box, due_at, attempts) VALUES(?,?,?,?,?,?,?,?,?,?)",
                &[&course.id, &ui, &ut, &ci, &title, &summary, &mastery, &bx, &due, &att]);
        }
    }
    if let Some(e) = data["emoji"].as_str() {
        if course.emoji == "📘" && !e.is_empty() {
            let e: String = e.chars().take(4).collect();
            db.exec("UPDATE courses SET emoji=? WHERE id=?", &[&e, &course.id]);
        }
    }
}

// ---------- test prep ----------

pub fn test_context(t: &Test) -> String {
    let when = match crate::testprep::days_left(t) {
        Some(0) => "today".to_string(),
        Some(1) => "tomorrow".to_string(),
        Some(n) if n > 0 => format!("in {n} days"),
        Some(_) => "date passed".to_string(),
        None => "date not set".to_string(),
    };
    let mut s = format!("Upcoming test: {} on {} ({when}).", t.name, t.test_date.clone().unwrap_or_else(|| "an unset date".into()));
    if !t.topics.is_empty() {
        s += &format!("\nWhat the learner says is on the test:\n{}", t.topics);
    }
    if !t.overview.is_empty() {
        s += &format!("\nTest overview: {}", t.overview);
    }
    s
}

pub fn test_schema() -> Value {
    json!({"type": "object", "properties": {
        "overview": {"type": "string"}, "tips": {"type": "array", "items": {"type": "string"}},
        "concepts": {"type": "array", "items": {"type": "object", "properties": {
            "existing_concept_id": {"type": "integer"}, "title": {"type": "string"}, "summary": {"type": "string"},
            "priority": {"type": "string", "enum": ["high", "medium", "low"]}, "why": {"type": "string"}},
            "required": ["existing_concept_id", "title", "summary", "priority", "why"], "additionalProperties": false}}},
        "required": ["overview", "tips", "concepts"], "additionalProperties": false})
}

pub fn analyze_test(db: &Db, t: &Test, course: &Course, ids: &[i64]) -> Result<Value, LlmError> {
    for id in ids {
        materials::ensure_ready(db, *id);
    }
    // search the textbook with the test topics plus the text of the learner's quizzes / study guides
    let mut query = format!("{} {}", t.name, t.topics);
    for id in ids {
        if let Some(m) = Material::get(db, *id) {
            if !materials::is_large(&m) && (m.kind == "pdf" || m.kind == "text") {
                query.push(' ');
                query.extend(materials::whole_text(db, &m).chars().take(4000));
            }
        }
    }
    let (parts, _) = material_parts(db, ids, Some(&query), 24, true, &Exclude::new());
    let existing = Concept::for_course(db, course.id);
    let elist = if existing.is_empty() { "(none yet)".to_string() } else { existing.iter().map(|c| format!("- id {}: {} - {}", c.id, c.title, c.summary)).collect::<Vec<_>>().join("\n") };
    let instruction = format!("{}\n\n{}\n\nThe learner wants to be fully prepared for this test. Using the study materials above (look especially for study guides, review sheets, past exams, quizzes and syllabus/test-coverage notes; for long textbooks you have the table of contents and the pages most related to the test) and what they said is on the test, work out exactly what they need to know.

- \"concepts\": every concept likely to be tested, in the order they should be learned (foundations first). Each is one teachable, testable idea. Usually 5-20 depending on the test's scope.
  - \"existing_concept_id\": if it is the same idea as one of the course's existing concepts below, use that id; otherwise 0.
  - \"title\" 2-6 words; \"summary\" 1-2 sentences on what the learner must be able to do for the test, naming the textbook section if known.
  - \"priority\": high = central/heavily weighted/certain to appear; medium = likely; low = possible or minor.
  - \"why\": one short sentence on why it's on the test, citing the evidence (e.g. \"Quiz 3 Q2; textbook Section 4.7\").
- \"overview\": 2-4 sentences: what the test covers, its likely format and question styles, and what will matter most.
- \"tips\": 3-5 specific, practical prep tips for this particular test (common traps, what to memorize vs understand, problem types to drill).

Existing course concepts:
{elist}", course_header(db, course), test_context(t));
    Ok(json_job(db, "read", parts, instruction, &test_schema(), "high")?.0)
}

// ---------- questions ----------

pub fn quiz_schema() -> Value {
    json!({"type": "object", "properties": {"questions": {"type": "array", "items": {"type": "object", "properties": {
        "type": {"type": "string", "enum": ["multiple_choice", "true_false", "fill_blank", "match_pairs", "order_steps", "short_answer"]},
        "concept_id": {"type": "integer"}, "prompt": {"type": "string"},
        "options": {"type": "array", "items": {"type": "string"}}, "correct_index": {"type": "integer"},
        "accepted_answers": {"type": "array", "items": {"type": "string"}},
        "pairs": {"type": "array", "items": {"type": "object", "properties": {"left": {"type": "string"}, "right": {"type": "string"}},
            "required": ["left", "right"], "additionalProperties": false}},
        "ordered_items": {"type": "array", "items": {"type": "string"}},
        "model_answer": {"type": "string"}, "rubric": {"type": "string"}, "hint": {"type": "string"}, "explanation": {"type": "string"}},
        "required": ["type", "concept_id", "prompt", "options", "correct_index", "accepted_answers", "pairs", "ordered_items", "model_answer", "rubric", "hint", "explanation"],
        "additionalProperties": false}}}, "required": ["questions"], "additionalProperties": false})
}

fn quiz_kind(kind: &str) -> &'static str {
    match kind {
        "diagnostic" => "This is a quick diagnostic before teaching. Write questions that reveal whether the learner already understands this concept well enough for the test: one recall question, the rest application at the level the test will ask. Don't make them trivially easy, because passing means the learner skips the lesson.",
        "mock" => "This is a full mock exam. Simulate the real test as closely as possible: cover the concepts in proportion to their priority, mirror the format, wording and difficulty of past exams or the study guide, and include multi-step problems. Order from easier to harder.",
        _ => "This is test-prep practice. Write questions at the difficulty and in the style of the actual test (mirror any past exams, sample questions or study-guide items in the materials). Ramp up from solid fundamentals to test-level questions.",
    }
}

pub const QUESTION_FORMAT: &str = r#"Use a mix of types, filling only the fields that type uses (leave the others as empty lists / 0 / ""):
- multiple_choice: options (3-4 plausible choices, distractors based on real misconceptions), correct_index.
- true_false: options exactly ["True", "False"], correct_index.
- fill_blank: prompt contains "___" where the answer goes; accepted_answers lists every acceptable short answer (synonyms, notation variants, numbers with/without units). Keep answers to a word, number, or short phrase.
- match_pairs: 3-5 pairs (term -> definition, formula -> name, cause -> effect). Keep each side short.
- order_steps: ordered_items in the correct order (3-6 steps of a process, proof or procedure); the app shuffles them.
- short_answer: an open question the learner answers in their own words: explain why, justify, prove a small claim, or show a short calculation (1-5 sentences or lines). model_answer: a complete correct answer. rubric: the 2-4 key points a full-credit answer must contain. An AI grades it with partial credit. Use for roughly 1 in 5 questions (more in test practice and mock exams, where real tests ask for explanations and proofs).
Use Markdown/LaTeX ($...$) in prompts where helpful. "hint": a nudge that doesn't give the answer away. "explanation": 1-3 sentences on why the answer is right, naming the misconception behind the likeliest wrong answer, and citing where in the materials it's covered if applicable."#;

pub fn valid_question(q: &Value) -> bool {
    let n = |k: &str| q[k].as_array().map(|a| a.len()).unwrap_or(0);
    match q["type"].as_str() {
        Some("multiple_choice" | "true_false") => {
            let ci = q["correct_index"].as_i64().unwrap_or(-1);
            n("options") >= 2 && ci >= 0 && (ci as usize) < n("options")
        }
        Some("fill_blank") => n("accepted_answers") >= 1,
        Some("match_pairs") => n("pairs") >= 2,
        Some("order_steps") => n("ordered_items") >= 2,
        Some("short_answer") => q["model_answer"].as_str().map(|s| !s.is_empty()).unwrap_or(false),
        _ => false,
    }
}

fn normalize(mut q: Value) -> Value {
    for (k, v) in [("options", json!([])), ("accepted_answers", json!([])), ("pairs", json!([])), ("ordered_items", json!([])),
        ("correct_index", json!(0)), ("model_answer", json!("")), ("rubric", json!("")), ("hint", json!("")), ("explanation", json!(""))] {
        if q.get(k).map(|x| x.is_null()).unwrap_or(true) {
            q[k] = v;
        }
    }
    q
}

pub fn generate_quiz(db: &Db, course: &Course, concepts: &[Concept], count: usize, review: bool, test: Option<&Test>, kind: Option<&str>, material_ids: Option<Vec<i64>>) -> Result<(Vec<Value>, Provider), LlmError> {
    let ids = material_ids.filter(|v| !v.is_empty()).unwrap_or_else(|| Material::enabled_ids(db, course.id));
    let query: String = concepts.iter().map(|c| format!("{} {}", c.title, c.summary)).collect::<Vec<_>>().join(" ");
    let (parts, _) = material_parts(db, &ids, Some(&query), if kind == Some("mock") { 24 } else { 10 }, true, &Exclude::new());
    let clist = concepts.iter().map(|c| format!("- id {}: {} - {} (mastery {}/100)", c.id, c.title, c.summary, c.mastery as i64)).collect::<Vec<_>>().join("\n");
    let focus = match test {
        Some(t) => format!("{}\n{}", test_context(t), quiz_kind(kind.unwrap_or("practice"))),
        None if review => "This is a spaced-review session: interleave the concepts and focus on what is easy to forget.".into(),
        None => "Spread questions across the concepts; spend more on lower-mastery ones.".into(),
    };
    let difficulty = if test.is_some() { "" } else { "Difficulty: match mastery - recognition and basic recall for low mastery, application, multi-step problems and tricky distinctions for high mastery. Ramp up gently through the session." };
    let instruction = format!("{}\n\nWrite a {count}-question practice session in the style of Duolingo: quick, varied, satisfying questions that make the learner actively retrieve and apply knowledge. Base them on the study materials above (reuse the style of any quizzes or problem sets in them).\n\nConcepts to cover (use these ids for concept_id):\n{clist}\n\n{focus}\n{difficulty}\n\n{QUESTION_FORMAT}", course_header(db, course));
    let job = if kind == Some("mock") { "exam" } else { "quiz" };
    let (data, via) = json_job(db, job, parts, instruction, &quiz_schema(), "medium")?;
    let valid: HashSet<i64> = concepts.iter().map(|c| c.id).collect();
    let out: Vec<Value> = data["questions"]
        .as_array()
        .into_iter()
        .flatten()
        .cloned()
        .map(normalize)
        .map(|mut q| {
            if !valid.contains(&q["concept_id"].as_i64().unwrap_or(-1)) {
                q["concept_id"] = json!(concepts.first().map(|c| c.id).unwrap_or(0));
            }
            q
        })
        .filter(valid_question)
        .collect();
    if out.is_empty() {
        return Err(LlmError::BadOutput("no usable questions".into()));
    }
    Ok((out, via))
}

/// One fresh question per missed question, aimed at the same misconception.
pub fn generate_variants(db: &Db, course: &Course, mistakes: &[crate::model::Mistake]) -> Result<(Vec<Value>, Provider), LlmError> {
    let listing = mistakes
        .iter()
        .enumerate()
        .map(|(i, m)| format!("Mistake {} (concept_id {}):\nQuestion: {}\nLearner's wrong answer: {}", i + 1, m.concept_id.unwrap_or(0), m.question, m.user_answer))
        .collect::<Vec<_>>()
        .join("\n\n");
    let instruction = format!("Course: {}\n\nThese are questions the learner got wrong before. Write exactly {} questions, one per mistake and in the same order, so question i is a fresh variant of mistake i: it tests the same idea and specifically targets the misconception the wrong answer reveals, but with different wording, numbers or examples (and a different question type if that suits the idea better). Don't reuse the original question verbatim. Keep concept_id the same as the mistake's.\n\n{listing}\n\n{QUESTION_FORMAT}", course.name, mistakes.len());
    let (data, via) = json_job(db, "variants", vec![], instruction, &quiz_schema(), "medium")?;
    let out: Vec<Value> = data["questions"]
        .as_array()
        .into_iter()
        .flatten()
        .take(mistakes.len())
        .enumerate()
        .map(|(i, q)| {
            let mut q = normalize(q.clone());
            q["concept_id"] = json!(mistakes[i].concept_id.unwrap_or(0));
            q["mistake_id"] = json!(mistakes[i].id);
            q
        })
        .filter(valid_question)
        .collect();
    if out.is_empty() {
        return Err(LlmError::BadOutput("no usable questions".into()));
    }
    Ok((out, via))
}

pub fn grade(db: &Db, q: &Value, answer: &str) -> Result<(Value, Provider), LlmError> {
    let schema = json!({"type": "object", "properties": {"verdict": {"type": "string", "enum": ["correct", "partial", "incorrect"]},
        "score": {"type": "number"}, "feedback": {"type": "string"}}, "required": ["verdict", "score", "feedback"], "additionalProperties": false});
    let answer: String = answer.chars().take(4000).collect();
    let instruction = format!("Grade the learner's written answer to this practice question. Be fair and specific, like a good teaching assistant.

Question: {}
Model answer: {}
Key points for full credit: {}

Learner's answer:
\"\"\"{answer}\"\"\"

- Judge the ideas, not the wording; an answer that reaches the right result by a different valid method gets full credit.
- verdict \"correct\" = all key points present and no real errors; \"partial\" = right idea but missing a key point or containing a fixable error; \"incorrect\" = wrong or missing the main idea.
- score: 0 to 1.
- feedback: 1-3 sentences addressed to the learner (\"you\"). Say what they got right, then point to the exact step or point that's missing or wrong. Use LaTeX ($...$) for math.",
        q["prompt"].as_str().unwrap_or(""), q["model_answer"].as_str().unwrap_or(""), q["rubric"].as_str().filter(|s| !s.is_empty()).unwrap_or("(use the model answer)"));
    let (mut v, via) = llm::json_call(db, "grade", "You are a fair, precise grader for a tutoring app.", &[Msg::user(vec![Part::text(instruction)])], &schema, 6000, "low")?;
    v["score"] = json!(v["score"].as_f64().unwrap_or(0.0).clamp(0.0, 1.0));
    Ok((v, via))
}

pub fn explain(db: &Db, q: &Value, user_answer: &str, on: &mut dyn FnMut(Ev)) -> Result<Provider, LlmError> {
    let prompt = format!("A learner just got this practice question wrong and tapped \"Explain this\". Explain it so it clicks: what the question is really asking, why the correct answer is right, exactly what's off about their answer (and why it's a tempting mistake), and a quick memory trick or rule of thumb. Keep it short and friendly, under 200 words.\n\nQuestion (JSON): {q}\nLearner's answer: {user_answer}");
    Ok(llm::stream(db, "explain", SYSTEM, &[Msg::user(vec![Part::text(prompt)])], false, on)?.provider)
}

// ---------- lessons ----------

/// Stored conversation -> provider-neutral messages.
fn to_messages(db: &Db, history: &[Value], snapshot: &[i64]) -> Vec<Msg> {
    history
        .iter()
        .map(|m| {
            let content = m["content"].as_array().cloned().unwrap_or_default();
            if m["role"] == "assistant" {
                let text: String = content.iter().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect();
                return Msg { user: false, parts: vec![Part::text(text)], raw: Some(Value::Array(content)) };
            }
            let mut parts = vec![];
            for b in &content {
                match b["type"].as_str() {
                    Some("lumi_materials") => parts.extend(material_parts(db, snapshot, None, 0, true, &Exclude::new()).0),
                    Some("lumi_pages") => {
                        for r in b["refs"].as_array().into_iter().flatten() {
                            let mid = r[0].as_i64().unwrap_or(0);
                            let pages: Vec<u32> = r[1].as_array().into_iter().flatten().filter_map(|p| p.as_u64().map(|x| x as u32)).collect();
                            if let Some(mat) = Material::get(db, mid) {
                                parts.push(excerpt_part(db, &mat, &pages));
                            }
                        }
                    }
                    Some("lumi_image") => {
                        let name = b["name"].as_str().unwrap_or("");
                        let p = db.path("chat_images").join(std::path::Path::new(name).file_name().unwrap_or_default());
                        if let Ok(data) = std::fs::read(p) {
                            parts.push(Part::text("[Photo the learner just attached to this message:]"));
                            parts.push(Part::Image { data, mime: b["media_type"].as_str().unwrap_or("image/jpeg").into() });
                        }
                    }
                    Some("text") => parts.push(Part::text(b["text"].as_str().unwrap_or(""))),
                    _ => {}
                }
            }
            Msg { user: true, parts, raw: None }
        })
        .collect()
}

fn seen_pages(history: &[Value]) -> Exclude {
    let mut seen = Exclude::new();
    for m in history.iter().filter(|m| m["role"] == "user") {
        for b in m["content"].as_array().into_iter().flatten().filter(|b| b["type"] == "lumi_pages") {
            for r in b["refs"].as_array().into_iter().flatten() {
                let e = seen.entry(r[0].as_i64().unwrap_or(0)).or_default();
                e.extend(r[1].as_array().into_iter().flatten().filter_map(|p| p.as_u64().map(|x| x as u32)));
            }
        }
    }
    seen
}

pub struct ChatDone {
    pub events: game::Events,
    pub via: Provider,
    pub truncated: bool,
}

/// One lesson turn: pulls up relevant textbook pages, streams the reply, saves the conversation.
/// `images` are already-saved chat images: (file name, media type).
pub fn chat_turn(db: &Db, sid: i64, user_text: &str, images: &[(String, String)], on: &mut dyn FnMut(Ev)) -> Result<ChatDone, LlmError> {
    let Some(session) = Session::get(db, sid) else { return Err(LlmError::BadOutput("lesson not found".into())) };
    let course = Course::get(db, session.course_id).unwrap_or_default();
    let mut history = session.messages.clone();
    let concept = session.concept_id.and_then(|id| Concept::get(db, id));
    let query = match (&concept, history.is_empty()) {
        (Some(c), true) => format!("{} {} {user_text}", c.title, c.summary),
        _ => user_text.to_string(),
    };
    let (_, refs) = material_parts(db, &session.material_ids, Some(&query), if history.is_empty() { 10 } else { 4 }, false, &seen_pages(&history));
    let mut blocks: Vec<Value> = vec![];
    if history.is_empty() {
        blocks.push(json!({"type": "lumi_materials"}));
    }
    if !refs.is_empty() {
        blocks.push(json!({"type": "lumi_pages", "refs": refs.iter().map(|(m, p)| json!([m, p])).collect::<Vec<_>>()}));
    }
    for (name, mt) in images {
        blocks.push(json!({"type": "lumi_image", "name": name, "media_type": mt}));
    }
    let text = if history.is_empty() {
        let mut focus = match &concept {
            Some(c) => format!("This lesson's concept: {} - {} (learner's current mastery {}/100).", c.title, c.summary, c.mastery as i64),
            None => "This is an open study session: help with whatever the learner asks about the course - explaining concepts, walking through problems from the materials, or reviewing.".into(),
        };
        if let Some(t) = session.test_id.and_then(|id| Test::get(db, id)) {
            focus += &format!("\n\n{}\nThis is test prep: the learner is about to be tested on this concept. Teach efficiently toward what the test will ask: diagnose gaps quickly, explain, then drill with test-style questions (mirror past exams or the study guide where available). Point out common traps and what to memorize. When they can handle test-level questions, tell them they're ready to tap \"Ready to practice\".", test_context(&t));
        }
        format!("<lesson_context>\n{}\n\n{focus}\n</lesson_context>\n\n{user_text}", course_header(db, &course))
    } else {
        user_text.to_string()
    };
    blocks.push(json!({"type": "text", "text": text}));
    let user_msg = json!({"role": "user", "content": blocks});
    let mut all = history.clone();
    all.push(user_msg.clone());
    let out = llm::stream(db, "teach", SYSTEM, &to_messages(db, &all, &session.material_ids), true, on)?;
    let (content, truncated) = match out.claude {
        Some((c, stop)) => (c, stop == "max_tokens"),
        None => (json!([{"type": "text", "text": out.text}]), false),
    };
    history.push(user_msg);
    history.push(json!({"role": "assistant", "content": content, "via": out.provider}));
    let s = serde_json::to_string(&history).unwrap();
    db.exec("UPDATE sessions SET messages=?, updated_at=datetime('now') WHERE id=?", &[&s, &sid]);
    game::quest_bump(db, "lesson_msgs", 1);
    let mut events = game::award(db, 3, 0, 0);
    if history.len() == 2 {
        game::unlock(db, "curious_mind", &mut events);
    }
    Ok(ChatDone { events, via: out.provider, truncated })
}

#[derive(Clone, Debug)]
pub struct DisplayMsg {
    pub user: bool,
    pub text: String,
    pub images: Vec<String>,
    pub pages: Vec<String>,
    pub via: Option<String>,
}

pub fn display_messages(db: &Db, s: &Session) -> Vec<DisplayMsg> {
    static CTX: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"(?s)<lesson_context>.*?</lesson_context>\s*").unwrap());
    s.messages
        .iter()
        .map(|m| {
            let content = m["content"].as_array().cloned().unwrap_or_default();
            let text: String = content.iter().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect();
            let user = m["role"] == "user";
            let pages = content
                .iter()
                .filter(|b| b["type"] == "lumi_pages")
                .flat_map(|b| b["refs"].as_array().cloned().unwrap_or_default())
                .filter_map(|r| {
                    let mat = Material::get(db, r[0].as_i64()?)?;
                    let pages: Vec<u32> = r[1].as_array()?.iter().filter_map(|p| p.as_u64().map(|x| x as u32)).collect();
                    Some(format!("{}: {}", mat.filename, materials::label_pages(&mat, &pages)))
                })
                .collect();
            DisplayMsg {
                user,
                text: if user { CTX.replace(&text, "").to_string() } else { text },
                images: content.iter().filter(|b| b["type"] == "lumi_image").filter_map(|b| b["name"].as_str().map(String::from)).collect(),
                pages,
                via: m["via"].as_str().map(|v| llm::label(v).to_string()),
            }
        })
        .collect()
}
