//! Inert Bank — demo data only, no real money, no real accounts.
//! Run: `cargo run -p inert-example-bank` then open `inert://bank.example`.

use inert_server::proto::Decimal;
use inert_server::{App, Bundle, Data, Next, Reject, val};

struct Account {
    id: &'static str,
    label: &'static str,
    cents: i64,
}

struct Tx {
    date: String,
    label: String,
    cents: i64,
}

struct Bank {
    accounts: Vec<Account>,
    txs: Vec<Tx>,
    notify: bool,
    currency: String,
}

impl Default for Bank {
    fn default() -> Self {
        Bank {
            accounts: vec![
                Account { id: "chk", label: "Everyday checking", cents: 4_218_40 },
                Account { id: "sav", label: "Savings", cents: 12_900_00 },
                Account { id: "trv", label: "Travel pot", cents: 640_15 },
            ],
            txs: vec![
                Tx { date: "2026-11-06".into(), label: "Coffee Lab".into(), cents: -4_80 },
                Tx { date: "2026-11-05".into(), label: "Salary — Demo GmbH".into(), cents: 3_250_00 },
                Tx { date: "2026-11-04".into(), label: "Filament supplier".into(), cents: -38_90 },
                Tx { date: "2026-11-03".into(), label: "Rent".into(), cents: -1_150_00 },
            ],
            notify: true,
            currency: "eur".into(),
        }
    }
}

fn code(c: &str) -> &'static str {
    match c {
        "usd" => "USD",
        "try" => "TRY",
        _ => "EUR",
    }
}

fn build(bundle: Bundle) -> anyhow::Result<App<Bank>> {
    let mut app = App::new(bundle, Bank::default());

    app.screen("overview", |b, _| {
        let total: i64 = b.accounts.iter().map(|a| a.cents).sum();
        Ok(Data::new()
            .set("holder", val::text("Demo Holder · DE89 •••• 3000"))
            .set("total", val::dec(total, 2))
            .set("currency", val::text(code(&b.currency)))
            .set(
                "accounts",
                val::list(b.accounts.iter().map(|a| val::obj([("id", val::text(a.id)), ("label", val::text(a.label)), ("balance", val::dec(a.cents, 2))]))),
            )
            .set(
                "transactions",
                val::list(b.txs.iter().take(12).map(|t| val::obj([("date", val::text(&t.date)), ("label", val::text(&t.label)), ("amount", val::dec(t.cents, 2))]))),
            ))
    });

    app.screen("transfer", |b, _| {
        Ok(Data::new()
            .set("accounts", val::list(b.accounts.iter().map(|a| val::obj([("id", val::text(a.id)), ("label", val::text(format!("{} — {}", a.label, Decimal::new(a.cents, 2))))]))))
            .set("limit", val::dec(10_000_00, 2)))
    });

    app.screen("settings", |b, _| Ok(Data::new().set("notify", val::bool(b.notify)).set("currency", val::text(&b.currency))));

    app.action("transfer", |b, a| {
        let to = a.text("to").trim().to_owned();
        let iban_like = to.len() >= 15 && to.chars().all(|c| c.is_ascii_alphanumeric() || c == ' ') && to.chars().take(2).all(|c| c.is_ascii_alphabetic());
        if !iban_like {
            return Err(Reject::on("to"));
        }
        let amount = a.decimal("amount").rescale(2).ok_or(Reject::on("amount"))?.mantissa;
        let from = a.choice("from");
        let acct = b.accounts.iter_mut().find(|x| x.id == from).ok_or(Reject::on("from"))?;
        if acct.cents < amount {
            return Err(Reject::on("amount"));
        }
        acct.cents -= amount;
        let note = a.text("note").trim();
        let label = if note.is_empty() { format!("Transfer to {}", &to[..to.len().min(12)]) } else { format!("{note} → {}", &to[..to.len().min(8)]) };
        b.txs.insert(0, Tx { date: today(), label: label.chars().take(60).collect(), cents: -amount });
        Ok(Next::screen("overview"))
    });

    app.action("save_settings", |b, a| {
        b.notify = a.bool("notify");
        b.currency = a.choice("currency").to_owned();
        Ok(Next::screen("overview"))
    });

    Ok(app)
}

fn main() -> anyhow::Result<()> {
    inert_server::dev::main(env!("CARGO_MANIFEST_DIR"), "127.0.0.1:7443", build)
}

/// Today's date (UTC) as YYYY-MM-DD, so new transfers are dated today.
fn today() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}
