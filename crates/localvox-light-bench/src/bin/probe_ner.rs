//! Probe: THE GROUNDING CHECK with a live NER model, on real scenarios.
//!
//! We check two properties at once, because each of the failures is costly:
//!   * is an INVENTION caught (that very acceptance accident: out of humming a summary was born
//!     with action items for "Иван" and "150 задачами");
//!   * is an HONEST retelling not accused (a barrage of false alarms is the second trouble, the
//!     one that sent summaries into quarantine).
//!
//! Run: `cargo run -p localvox-light-bench --bin probe_ner`

use localvox_light_core::ner::{model_dir, Ner};
use localvox_light_llm::grounding::{self, Entities};

struct Gliner {
    ner: Ner,
    labels: Vec<String>,
}

impl Entities for Gliner {
    fn people(&self, text: &str) -> Vec<String> {
        self.ner
            .extract(text, &self.labels)
            .map(|v| v.into_iter().map(|e| e.text).collect())
            .unwrap_or_default()
    }
    fn people_in_source(&self, text: &str) -> Vec<String> {
        self.ner
            .extract_at(text, &self.labels, 0.3)
            .map(|v| v.into_iter().map(|e| e.text).collect())
            .unwrap_or_default()
    }
}

fn main() -> anyhow::Result<()> {
    let dir = model_dir().ok_or_else(|| anyhow::anyhow!("no NER model directory"))?;
    let ner = Gliner {
        ner: Ner::open(&dir)?,
        labels: ["имя человека"].iter().map(|s| s.to_string()).collect(),
    };
    let lex = localvox_light_core::lexicon::active();

    // TWO INDEPENDENT PASSES: the name (recall) and the role (filter).
    // One bucket takes weight away from the other — which means they must be asked separately.
    {
        let answers = [
            "Говорящий утверждает, что оборудование не импортное.",
            "Рассказчик отработал на заводе три года.",
            "Спикер рассказывает о работе на заводе.",
            "Автор записи делится наблюдениями.",
            "Роман закроет задачу к концу года.",
            "Достоевский заклеймил Обломова как лишнего человека.",
            "Marcus will ship the feature on Friday.",
            "Иван готовит смету, Мария закрывает бэклог.",
        ];
        let name = vec!["имя человека".to_string()];
        let role = vec!["говорящий или роль".to_string()];
        println!(
            "{:<52} {:<28} {}",
            "answer", "NAME (pass A)", "ROLE (pass B)"
        );
        for a in answers {
            let n: Vec<String> = ner
                .ner
                .extract_at(a, &name, 0.4)
                .unwrap()
                .iter()
                .map(|x| format!("{}({:.2})", x.text, x.score))
                .collect();
            let r: Vec<String> = ner
                .ner
                .extract_at(a, &role, 0.4)
                .unwrap()
                .iter()
                .map(|x| format!("{}({:.2})", x.text, x.score))
                .collect();
            println!(
                "{:<52} {:<28} {}",
                a.chars().take(50).collect::<String>(),
                n.join(" "),
                r.join(" ")
            );
        }
        println!();
    }

    // (speech, model's answer, do we expect an accusation)
    let cases: [(&str, &str, bool); 6] = [
        // 1. THAT VERY ACCIDENT: almost nothing in the recording, people and numbers in the summary.
        (
            "Так. Ну что. Угу. Ла-ла-ла. Ага.",
            "## Решения\n- Иван готовит отчёт.\n- Мария закрывает 150 задач в бэклоге.",
            true,
        ),
        // 2. An honest Russian summary — there is nothing to accuse.
        (
            "Мы с Иваном обсудили бюджет на следующий квартал, там около двухсот тысяч рублей.",
            "## О чём запись\nОбсудили с Иваном бюджет на следующий квартал — порядка 200 000 рублей.",
            false,
        ),
        // 3. HOMONYMY: in the recording «роман» is a book, in the summary «Роман» is a person.
        (
            "Обсудили планы: надо дописать роман к концу года.",
            "## Задачи\n- Роман закроет задачу к концу года.",
            true,
        ),
        // 4. The same one, but honest: the novel stayed a book.
        (
            "Обсудили планы: надо дописать роман к концу года.",
            "## О чём запись\nРешили дописать роман к концу года.",
            false,
        ),
        // 5. An honest ENGLISH retelling — the one that used to produce six false accusations.
        (
            "So we discussed the release timeline and agreed to postpone the rollout to next week \
             because the tests are still failing.",
            "## Key points\n- The team decided to delay deployment until the following week, \
             since the test suite remains unstable.",
            false,
        ),
        // 6. An English invention: there was no such person in the recording.
        (
            "We agreed to ship the feature next week and to tell the team about it.",
            "## Tasks\n- Marcus will ship the feature on Friday.",
            true,
        ),
    ];

    let mut wrong = 0;
    for (i, (speech, answer, expect_flag)) in cases.iter().enumerate() {
        let t = std::time::Instant::now();
        let u = grounding::check_with_entities(lex, speech, speech, answer, &ner);
        let flagged = !u.is_empty();
        let ok = flagged == *expect_flag;
        if !ok {
            wrong += 1;
        }
        println!(
            "{} {}. expected {:<10} got {:<10} [{:.0} ms]  {}",
            if ok { "✔" } else { "✘" },
            i + 1,
            if *expect_flag {
                "accusation"
            } else {
                "silence"
            },
            if flagged {
                "accusation"
            } else {
                "silence"
            },
            t.elapsed().as_millis(),
            u.describe()
        );
        if !ok {
            println!("      PEOPLE in the source: {:?}", ner.people_in_source(speech));
            println!("      PEOPLE in the answer: {:?}", ner.people(answer));
        }
    }
    println!("\ncorrect: {}/{}", cases.len() - wrong, cases.len());
    Ok(())
}
