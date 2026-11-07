//! Inert Notes — notes and tasks with demo data.
//! Run: `cargo run -p inert-example-notes` then open `inert://notes.example`.

use inert_server::{App, Bundle, Data, Fields, Next, Reject, val};

struct Note {
    id: String,
    title: String,
    body: String,
    category: String,
    done: bool,
    rev: u32,
}

struct Notes {
    notes: Vec<Note>,
    next: u32,
}

impl Default for Notes {
    fn default() -> Self {
        let n = |id: &str, t: &str, b: &str, c: &str, d: bool| Note { id: id.into(), title: t.into(), body: b.into(), category: c.into(), done: d, rev: 1 };
        Notes {
            notes: vec![
                n("n1", "Write the Inert spec", "Wire format, manifest, verification rules, limits.", "work", true),
                n("n2", "Print a new lamp shade", "PETG, 0.2 mm layers, vase mode.", "personal", false),
                n("n3", "Agents read the tree directly", "Same semantic tree for screen readers and AI agents.", "ideas", false),
            ],
            next: 4,
        }
    }
}

fn build(bundle: Bundle) -> anyhow::Result<App<Notes>> {
    let mut app = App::new(bundle, Notes::default());

    app.screen("list", |s, _| {
        Ok(Data::new().set("open", val::int(s.notes.iter().filter(|n| !n.done).count() as i64)).set(
            "notes",
            val::list(s.notes.iter().map(|n| {
                val::obj([
                    ("id", val::text(&n.id)),
                    ("label", val::text(&n.title)),
                    ("category", val::text(n.category.to_uppercase())),
                    ("status", val::text(if n.done { "[x]" } else { "[ ]" })),
                ])
            })),
        ))
    });

    app.screen("note", |s, p| {
        let n = s.notes.iter().find(|n| n.id == p.text("id")).ok_or(Reject::general())?;
        Ok(Data::new()
            .set("id", val::text(&n.id))
            .set("title", val::text(&n.title))
            .set("body", val::text(&n.body))
            .set("category", val::text(&n.category))
            .set("done", val::bool(n.done))
            .set("updated", val::text(format!("revision {}", n.rev))))
    });

    app.action("add_note", |s, a| {
        let title = a.text("title").trim();
        if title.is_empty() {
            return Err(Reject::on("title"));
        }
        let id = format!("n{}", s.next);
        s.next += 1;
        s.notes.insert(0, Note { id, title: title.to_owned(), body: String::new(), category: a.choice("category").to_owned(), done: false, rev: 1 });
        Ok(Next::Stay)
    });

    app.action("delete_note", |s, a| {
        let id = a.choice("id").to_owned();
        s.notes.retain(|n| n.id != id);
        Ok(Next::Stay)
    });

    app.action("save_note", |s, a| {
        let id = a.text("id").to_owned();
        let n = s.notes.iter_mut().find(|n| n.id == id).ok_or(Reject::general())?;
        let title = a.text("title").trim();
        if title.is_empty() {
            return Err(Reject::on("title"));
        }
        n.title = title.to_owned();
        n.body = a.text("body").to_owned();
        n.category = a.choice("category").to_owned();
        n.done = a.bool("done");
        n.rev += 1;
        let mut p = Fields::new();
        p.insert("id".into(), val::text(id));
        Ok(Next::Screen("note".into(), p))
    });

    Ok(app)
}

fn main() -> anyhow::Result<()> {
    inert_server::dev::main(env!("CARGO_MANIFEST_DIR"), "127.0.0.1:7445", build)
}
