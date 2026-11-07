//! Inert Supply — a demo store. No payments, nothing ships.
//! Run: `cargo run -p inert-example-shop` then open `inert://shop.example`.

use inert_server::proto::Hash;
use inert_server::{App, Bundle, Data, Next, Reject, val};

struct Product {
    id: &'static str,
    name: &'static str,
    cents: i64,
    description: &'static str,
    image: Hash,
}

struct Line {
    id: String,
    product: &'static str,
    size: String,
    qty: i64,
}

#[derive(Default)]
struct Shop {
    products: Vec<Product>,
    lines: Vec<Line>,
    next_line: u32,
    orders: u32,
    last_total: i64,
}

impl Shop {
    fn product(&self, id: &str) -> Option<&Product> {
        self.products.iter().find(|p| p.id == id)
    }
    fn total_cents(&self) -> i64 {
        self.lines.iter().map(|l| self.product(l.product).map_or(0, |p| p.cents) * l.qty).sum()
    }
}

/// A dithered sphere, in the Stencil style, as a PNG.
fn sphere_png(size: u32, fg: [u8; 3], edge: [u8; 3], light: (f32, f32)) -> Vec<u8> {
    const BAYER: [[f32; 4]; 4] = [[0., 8., 2., 10.], [12., 4., 14., 6.], [3., 11., 1., 9.], [15., 7., 13., 5.]];
    let mut px = Vec::with_capacity((size * size * 3) as usize);
    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 - size as f32 / 2.0 + 0.5) / (size as f32 * 0.42);
            let dy = (y as f32 - size as f32 / 2.0 + 0.5) / (size as f32 * 0.42);
            let r = (dx * dx + dy * dy).sqrt();
            let c = if r > 1.0 {
                // sparse stars
                let h = (x.wrapping_mul(73856093) ^ y.wrapping_mul(19349663)) % 997;
                if h == 0 { [200, 200, 200] } else { [8, 8, 8] }
            } else if r > 0.96 {
                edge
            } else {
                let lit = (0.5 + light.0 * dx + light.1 * dy + 0.3 * (1.0 - r)).clamp(0.0, 1.0);
                if lit > (BAYER[(y % 4) as usize][(x % 4) as usize] + 0.5) / 16.0 { fg } else { [8, 8, 8] }
            };
            px.extend_from_slice(&c);
        }
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, size, size);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(&px).unwrap();
    }
    out
}

fn build(bundle: Bundle) -> anyhow::Result<App<Shop>> {
    let mut app = App::new(bundle, Shop::default());
    let catalog = [
        ("lamp", "Orbit desk lamp", 89_00, "A warm, dimmable lamp with a printed shade. Demo product.", [0xff, 0xb5, 0x47], [0xff, 0xe0, 0xa0], (-0.6, 0.4)),
        ("mug", "Stoneware mug", 24_00, "Matte glaze, 350 ml, dishwasher safe. Demo product.", [0xd8, 0x5a, 0x30], [0xf0, 0x99, 0x7b], (0.5, 0.5)),
        ("pad", "Felt desk pad", 39_00, "Wool felt, 80 × 30 cm, keeps the desk quiet. Demo product.", [0x7f, 0x9c, 0xff], [0xb5, 0xd4, 0xf4], (-0.4, -0.5)),
        ("plant", "Small fern", 18_50, "Low light, low effort, in a recycled pot. Demo product.", [0x5d, 0xca, 0xa5], [0x9f, 0xe1, 0xcb], (0.6, -0.3)),
    ];
    for (id, name, cents, description, fg, edge, light) in catalog {
        let image = app.register_image(sphere_png(240, fg, edge, light))?;
        app.state().products.push(Product { id, name, cents, description, image });
    }

    app.screen("catalog", |s, _| {
        Ok(Data::new().set("count", val::int(s.lines.iter().map(|l| l.qty).sum())).set(
            "products",
            val::list(s.products.iter().map(|p| val::obj([("id", val::text(p.id)), ("name", val::text(p.name)), ("price", val::dec(p.cents, 2)), ("image", val::image(p.image))]))),
        ))
    });

    app.screen("product", |s, p| {
        let prod = s.product(p.text("id")).ok_or(Reject::general())?;
        Ok(Data::new()
            .set("id", val::text(prod.id))
            .set("name", val::text(prod.name))
            .set("price", val::dec(prod.cents, 2))
            .set("description", val::text(prod.description))
            .set("image", val::image(prod.image)))
    });

    app.screen("cart", |s, _| {
        let lines = s.lines.iter().filter_map(|l| {
            let p = s.product(l.product)?;
            Some(val::obj([
                ("id", val::text(&l.id)),
                ("label", val::text(format!("{} · {}", p.name, l.size.to_uppercase()))),
                ("qty", val::int(l.qty)),
                ("line_total", val::dec(p.cents * l.qty, 2)),
            ]))
        });
        Ok(Data::new().set("lines", val::list(lines.collect::<Vec<_>>())).set("total", val::dec(s.total_cents(), 2)))
    });

    app.screen("checkout", |s, _| Ok(Data::new().set("total", val::dec(s.total_cents(), 2))));

    app.screen("receipt", |s, _| Ok(Data::new().set("order", val::text(format!("INR-{:05}", 4200 + s.orders))).set("total", val::dec(s.last_total, 2))));

    app.action("add_to_cart", |s, a| {
        let product = a.text("product");
        let pid = s.product(product).map(|p| p.id).ok_or(Reject::on("product"))?;
        let size = a.choice("size").to_owned();
        let qty = a.int("qty");
        if let Some(l) = s.lines.iter_mut().find(|l| l.product == pid && l.size == size) {
            l.qty = (l.qty + qty).min(9);
        } else {
            s.next_line += 1;
            s.lines.push(Line { id: format!("l{}", s.next_line), product: pid, size, qty });
        }
        Ok(Next::screen("cart"))
    });

    app.action("remove_line", |s, a| {
        let id = a.choice("line").to_owned();
        s.lines.retain(|l| l.id != id);
        Ok(Next::Stay)
    });

    app.action("place_order", |s, a| {
        if a.text("name").trim().len() < 2 {
            return Err(Reject::on("name"));
        }
        if a.text("address").trim().len() < 6 {
            return Err(Reject::on("address"));
        }
        if s.lines.is_empty() {
            return Err(Reject::general());
        }
        let express = if a.choice("shipping") == "express" { 9_00 } else { 0 };
        s.last_total = s.total_cents() + express;
        s.lines.clear();
        s.orders += 1;
        Ok(Next::screen("receipt"))
    });

    Ok(app)
}

fn main() -> anyhow::Result<()> {
    inert_server::dev::main(env!("CARGO_MANIFEST_DIR"), "127.0.0.1:7444", build)
}
