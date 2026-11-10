pub type P2 = [f64; 2];

#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Rect { x: f64, y: f64, w: f64, h: f64 },
    Circle { cx: f64, cy: f64, r: f64 },
    Poly(Vec<P2>),
    Dilate { poly: Vec<P2>, r: f64 },
}

const CIRCLE_SEGMENTS: usize = 72;
const CORNER_SEGMENTS: usize = 8;

impl Shape {
    pub fn contour(&self, offset: f64) -> Vec<P2> {
        match self {
            Shape::Rect { x, y, w, h } => rect_contour(*x, *y, *w, *h, offset),
            Shape::Circle { cx, cy, r } => circle_contour(*cx, *cy, r + offset),
            Shape::Poly(pts) => poly_offset(pts, offset),
            Shape::Dilate { poly, r } => poly_offset(poly, offset + r),
        }
    }

    pub fn contains(&self, p: P2) -> bool {
        match self {
            Shape::Rect { x, y, w, h } => {
                p[0] >= *x && p[0] <= x + w && p[1] >= *y && p[1] <= y + h
            }
            Shape::Circle { cx, cy, r } => (p[0] - cx).hypot(p[1] - cy) <= *r,
            Shape::Poly(pts) => point_in_poly(pts, p),
            Shape::Dilate { poly, r } => point_in_poly(poly, p) || dist_to_closed(poly, p) <= *r,
        }
    }

    pub fn bounds(&self) -> (P2, P2) {
        match self {
            Shape::Rect { x, y, w, h } => ([*x, *y], [x + w, y + h]),
            Shape::Circle { cx, cy, r } => ([cx - r, cy - r], [cx + r, cy + r]),
            Shape::Poly(pts) => bounds_of(pts),
            Shape::Dilate { poly, r } => {
                let (lo, hi) = bounds_of(poly);
                ([lo[0] - r, lo[1] - r], [hi[0] + r, hi[1] + r])
            }
        }
    }

    pub fn inradius(&self) -> f64 {
        match self {
            Shape::Rect { w, h, .. } => w.min(*h) / 2.0,
            Shape::Circle { r, .. } => *r,
            _ => f64::MAX,
        }
    }
}

pub fn bounds_of(pts: &[P2]) -> (P2, P2) {
    let mut lo = [f64::MAX; 2];
    let mut hi = [f64::MIN; 2];
    for p in pts {
        for i in 0..2 {
            lo[i] = lo[i].min(p[i]);
            hi[i] = hi[i].max(p[i]);
        }
    }
    (lo, hi)
}

pub fn point_in_poly(pts: &[P2], p: P2) -> bool {
    if pts.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = pts.len() - 1;
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[j]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

pub fn signed_area(pts: &[P2]) -> f64 {
    let mut a = 0.0;
    for i in 0..pts.len() {
        let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
        a += p[0] * q[1] - q[0] * p[1];
    }
    a / 2.0
}

pub fn rect_contour(x: f64, y: f64, w: f64, h: f64, offset: f64) -> Vec<P2> {
    if offset <= 0.0 {
        let (x0, y0, x1, y1) = (x - offset, y - offset, x + w + offset, y + h + offset);
        return vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    }
    let corners = [
        ([x + w, y], -90.0f64),
        ([x + w, y + h], 0.0),
        ([x, y + h], 90.0),
        ([x, y], 180.0),
    ];
    let mut out = Vec::new();
    for (c, start) in corners {
        for k in 0..=CORNER_SEGMENTS {
            let a = (start + 90.0 * k as f64 / CORNER_SEGMENTS as f64).to_radians();
            out.push([c[0] + offset * a.cos(), c[1] + offset * a.sin()]);
        }
    }
    out
}

pub fn circle_contour(cx: f64, cy: f64, r: f64) -> Vec<P2> {
    (0..CIRCLE_SEGMENTS)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / CIRCLE_SEGMENTS as f64;
            [cx + r * a.cos(), cy + r * a.sin()]
        })
        .collect()
}

pub fn poly_offset(pts: &[P2], offset: f64) -> Vec<P2> {
    let mut p: Vec<P2> = pts.to_vec();
    if signed_area(&p) < 0.0 {
        p.reverse();
    }
    let n = p.len();
    let normal = |a: P2, b: P2| {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l = dx.hypot(dy).max(1e-12);
        [dy / l, -dx / l]
    };
    (0..n)
        .map(|i| {
            let n1 = normal(p[(i + n - 1) % n], p[i]);
            let n2 = normal(p[i], p[(i + 1) % n]);
            let k = offset / (1.0 + n1[0] * n2[0] + n1[1] * n2[1]).max(1e-6);
            [p[i][0] + (n1[0] + n2[0]) * k, p[i][1] + (n1[1] + n2[1]) * k]
        })
        .collect()
}

pub fn dist(a: P2, b: P2) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

pub fn closed_length(pts: &[P2]) -> f64 {
    (0..pts.len())
        .map(|i| dist(pts[i], pts[(i + 1) % pts.len()]))
        .sum()
}

pub fn dist_to_segment(p: P2, a: P2, b: P2) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 < 1e-18 {
        0.0
    } else {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0)
    };
    dist(p, [a[0] + dx * t, a[1] + dy * t])
}

pub fn dist_to_closed(pts: &[P2], p: P2) -> f64 {
    (0..pts.len())
        .map(|i| dist_to_segment(p, pts[i], pts[(i + 1) % pts.len()]))
        .fold(f64::MAX, f64::min)
}

pub fn dist_to_open(pts: &[P2], p: P2) -> f64 {
    if pts.len() == 1 {
        return dist(pts[0], p);
    }
    pts.windows(2)
        .map(|w| dist_to_segment(p, w[0], w[1]))
        .fold(f64::MAX, f64::min)
}

pub fn scan_span(poly: &[P2], y: f64) -> Option<(f64, f64)> {
    let mut lo = f64::MAX;
    let mut hi = f64::MIN;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        if (a[1] - y) * (b[1] - y) <= 0.0 && (a[1] - b[1]).abs() > 1e-12 {
            let x = a[0] + (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1]);
            lo = lo.min(x);
            hi = hi.max(x);
        } else if (a[1] - y).abs() < 1e-12 && (b[1] - y).abs() < 1e-12 {
            lo = lo.min(a[0].min(b[0]));
            hi = hi.max(a[0].max(b[0]));
        }
    }
    (lo <= hi).then_some((lo, hi))
}
