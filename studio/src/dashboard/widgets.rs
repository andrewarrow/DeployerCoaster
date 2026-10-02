use super::*;

pub(super) fn paint_product(
    ui: &Ui,
    rect: Rect,
    product: &Product,
    filter: Filter,
    play: &mut PlayStore,
    apple: &mut AppleStore,
) {
    let preferred = match filter {
        Filter::Android => Store::Play,
        Filter::All | Filter::Apple => Store::Apple,
    };
    let listing = product_icon_listing(product, preferred, |listing| match listing.store {
        Store::Apple => apple.has_dashboard_icon(&listing.identifier),
        Store::Play => play.has_dashboard_icon(&listing.identifier),
    });
    if let Some(listing) = listing {
        match listing.store {
            Store::Apple => {
                apple.paint_dashboard_icon(ui, rect, &listing.identifier, &product.name)
            }
            Store::Play => play.paint_dashboard_icon(ui, rect, &listing.identifier, &product.name),
        }
    }
}

pub(super) fn product_icon_listing(
    product: &Product,
    preferred: Store,
    mut has_icon: impl FnMut(&StoreApp) -> bool,
) -> Option<&StoreApp> {
    product
        .listings
        .iter()
        .find(|listing| listing.store == preferred && has_icon(listing))
        .or_else(|| product.listings.iter().find(|listing| has_icon(listing)))
        .or_else(|| {
            product
                .listings
                .iter()
                .find(|listing| listing.store == preferred)
        })
        .or_else(|| product.listings.first())
}

pub(super) fn refresh_control(ui: &mut Ui) -> Response {
    let (_, response) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "Refresh apps")
    });
    response.on_hover_text("Refresh apps")
}

pub(super) fn control(ui: &Ui, rect: Rect, id: impl egui::AsIdSalt, label: &str) -> Response {
    let response = ui.interact(rect, ui.id().with(id), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    response
}

pub(super) fn text(
    ui: &Ui,
    pos: Pos2,
    value: &str,
    size: f32,
    strong: bool,
    color: Color32,
    width: f32,
) {
    if width <= 0.0 {
        return;
    }
    let font = FontId::new(
        size,
        if strong {
            FontFamily::Name("semibold".into())
        } else {
            FontFamily::Proportional
        },
    );
    let mut job = egui::text::LayoutJob::simple(value.to_owned(), font, color, width);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    ui.painter().galley(pos, galley, color);
}

#[derive(Clone, Copy)]
pub(super) enum Icon {
    Home,
    Apps,
    Globe,
    Domain,
    Server,
    Shield,
    Chart,
    Deploy,
    Dollar,
    Users,
    Settings,
    Link,
    Box,
    Refresh,
    External,
    Chevron,
    Phone,
    Apple,
    Play,
    Activity,
}

impl Icon {
    pub(super) fn paint(self, ui: &Ui, origin: Pos2, size: f32, color: Color32) {
        let p = ui.painter();
        let point = |x: f32, y: f32| origin + vec2(x, y) * (size / 24.0);
        let stroke = Stroke::new((size / 14.0).max(1.1), color);
        let line = |coords: &[(f32, f32)]| {
            p.add(Shape::line(
                coords.iter().map(|&(x, y)| point(x, y)).collect(),
                stroke,
            ));
        };
        let rect = |x: f32, y: f32, w: f32, h: f32, radius: f32| {
            p.rect_stroke(
                Rect::from_min_size(point(x, y), vec2(w, h) * size / 24.0),
                radius,
                stroke,
                StrokeKind::Middle,
            );
        };
        match self {
            Self::Home => {
                line(&[(2., 11.), (12., 3.), (22., 11.)]);
                line(&[
                    (5., 9.),
                    (5., 21.),
                    (10., 21.),
                    (10., 14.),
                    (14., 14.),
                    (14., 21.),
                    (19., 21.),
                    (19., 9.),
                ]);
            }
            Self::Apps => {
                for (x, y) in [(3., 3.), (14., 3.), (3., 14.), (14., 14.)] {
                    rect(x, y, 7., 7., 1.5);
                }
            }
            Self::Globe | Self::Domain => {
                p.circle_stroke(point(12., 12.), size * 0.4, stroke);
                line(&[(3., 12.), (21., 12.)]);
                p.add(Shape::ellipse_stroke(
                    point(12., 12.),
                    vec2(size * 0.18, size * 0.4),
                    stroke,
                ));
            }
            Self::Server => {
                rect(3., 3., 18., 7., 1.5);
                rect(3., 14., 18., 7., 1.5);
                for y in [6.5, 17.5] {
                    p.circle_filled(point(7., y), size / 24., color);
                    line(&[(12., y), (17., y)]);
                }
            }
            Self::Shield => {
                line(&[
                    (12., 2.),
                    (21., 6.),
                    (20., 15.),
                    (17., 19.),
                    (12., 22.),
                    (7., 19.),
                    (4., 15.),
                    (3., 6.),
                    (12., 2.),
                ]);
                line(&[(8., 12.), (11., 15.), (16., 9.)]);
            }
            Self::Chart => {
                line(&[(3., 3.), (3., 21.), (22., 21.)]);
                line(&[(7., 15.), (11., 10.), (15., 13.), (21., 5.)]);
            }
            Self::Deploy => {
                rect(3., 5., 7., 6., 1.);
                rect(14., 14., 7., 6., 1.);
                line(&[(10., 8.), (17., 8.), (17., 14.)]);
                line(&[(14., 11.), (17., 14.), (20., 11.)]);
            }
            Self::Dollar => {
                line(&[
                    (17., 5.),
                    (8., 5.),
                    (5., 8.),
                    (8., 11.),
                    (16., 13.),
                    (19., 16.),
                    (16., 19.),
                    (6., 19.),
                ]);
                line(&[(12., 2.), (12., 22.)]);
            }
            Self::Users => {
                p.circle_stroke(point(10., 7.), size * 0.16, stroke);
                line(&[
                    (3., 21.),
                    (3., 17.),
                    (6., 14.),
                    (14., 14.),
                    (17., 17.),
                    (17., 21.),
                ]);
                line(&[(17., 3.), (20., 5.), (20., 9.), (18., 11.)]);
                line(&[(20., 15.), (23., 18.), (23., 21.)]);
            }
            Self::Settings => {
                p.circle_stroke(point(12., 12.), size * 0.17, stroke);
                let pts = (0..24)
                    .map(|i| {
                        let angle = i as f32 * std::f32::consts::TAU / 24.;
                        let r = if i % 4 < 2 { 10. } else { 8. };
                        point(12. + angle.cos() * r, 12. + angle.sin() * r)
                    })
                    .collect();
                p.add(Shape::closed_line(pts, stroke));
            }
            Self::Link => {
                line(&[
                    (10., 7.),
                    (14., 3.),
                    (18., 3.),
                    (21., 6.),
                    (21., 10.),
                    (17., 14.),
                ]);
                line(&[
                    (14., 17.),
                    (10., 21.),
                    (6., 21.),
                    (3., 18.),
                    (3., 14.),
                    (7., 10.),
                ]);
                line(&[(8., 16.), (16., 8.)]);
            }
            Self::Box => {
                line(&[
                    (3., 7.),
                    (12., 2.),
                    (21., 7.),
                    (21., 17.),
                    (12., 22.),
                    (3., 17.),
                    (3., 7.),
                    (12., 12.),
                    (21., 7.),
                ]);
                line(&[(12., 12.), (12., 22.)]);
            }
            Self::Refresh => {
                p.circle_stroke(point(12., 12.), size * 0.35, stroke);
                line(&[(20., 3.), (20., 9.), (14., 9.)]);
            }
            Self::External => {
                line(&[(13., 3.), (21., 3.), (21., 11.)]);
                line(&[(21., 3.), (11., 13.)]);
                line(&[(9., 4.), (4., 4.), (4., 20.), (20., 20.), (20., 15.)]);
            }
            Self::Chevron => line(&[(9., 5.), (15., 12.), (9., 19.)]),
            Self::Phone => {
                rect(5., 2., 14., 20., 2.);
                line(&[(10., 18.), (14., 18.)]);
            }
            Self::Apple => {
                line(&[(5., 19.), (18., 19.)]);
                line(&[(5., 16.), (13., 3.)]);
                line(&[(11., 3.), (19., 16.)]);
                line(&[(4., 14.), (20., 14.)]);
            }
            Self::Play => {
                p.add(Shape::convex_polygon(
                    vec![point(5., 2.), point(22., 12.), point(5., 22.)],
                    color,
                    Stroke::NONE,
                ));
            }
            Self::Activity => line(&[
                (1., 12.),
                (7., 12.),
                (10., 3.),
                (14., 21.),
                (17., 12.),
                (23., 12.),
            ]),
        }
    }
}
