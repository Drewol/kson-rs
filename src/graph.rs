use crate::*;

pub trait Graph<T> {
    fn value_at(&self, tick: f64) -> T;
    fn direction_at(&self, tick: f64) -> T;
}

impl Graph<f64> for Vec<GraphPoint> {
    fn value_at(&self, tick: f64) -> f64 {
        match self.binary_search_by(|g| g.y.cmp(&(tick as u32))) {
            Ok(i) =>
            //On a point
            {
                self.get(i).unwrap().v
            }
            Err(i) =>
            //Between points
            {
                if i == 0 {
                    self.first().map_or(0.0, |g| g.v)
                } else if i >= self.len() {
                    self.last().map_or(0.0, |g| g.vf.unwrap_or(g.v))
                } else {
                    let start_p = self.get(i - 1).unwrap();
                    let end_p = self.get(i).unwrap();
                    assert!(start_p.y < end_p.y);
                    let start_v = match start_p.vf {
                        Some(v) => v,
                        None => start_p.v,
                    };
                    let x = (tick - start_p.y as f64) / (end_p.y - start_p.y) as f64;
                    let width = end_p.v - start_v;
                    let (a, b) = match (start_p.a, start_p.b) {
                        (Some(a), Some(b)) => (a, b),
                        _ => (0., 0.),
                    };
                    if (a - b).abs() > f64::EPSILON {
                        start_v + do_curve(x, a, b) * width
                    } else {
                        start_v + x * width
                    }
                }
            }
        }
    }

    fn direction_at(&self, tick: f64) -> f64 {
        match self.binary_search_by(|g| g.y.cmp(&(tick as u32))) {
            Ok(i) =>
            //On a point
            {
                let point = self[i];
                let v = point.vf.unwrap_or(point.v);

                if let Some(next) = self.get(i + 1) {
                    (next.v - v) / (next.y - point.y) as f64
                } else {
                    0.0
                }
            }
            Err(i) =>
            //Between points
            {
                if i == 0 || i >= self.len() {
                    //Before the first point or after the last point
                    0.0
                } else {
                    let start_p = self.get(i - 1).unwrap();
                    let end_p = self.get(i).unwrap();
                    assert!(start_p.y < end_p.y);
                    let start_v = match start_p.vf {
                        Some(v) => v,
                        None => start_p.v,
                    };

                    (end_p.v - start_v) / (end_p.y - start_p.y) as f64
                }
            }
        }
    }
}

impl Graph<Option<f64>> for Vec<GraphSectionPoint> {
    fn value_at(&self, tick: f64) -> Option<f64> {
        match self.binary_search_by(|g| g.ry.cmp(&(tick as u32))) {
            Ok(p) /*On a point*/ => Some(self.get(p).unwrap().v),
            Err(p) /*Between points*/ => {
                if p == 0 || p >= self.len() {
                    return None;
                }
                let start_p = self.get(p - 1).unwrap();
                let end_p = self.get(p).unwrap();
                assert!(start_p.ry < end_p.ry);
                let start_v = match start_p.vf {
                    Some(v) => v,
                    None => start_p.v
                };
                let x = (tick - start_p.ry as f64) / (end_p.ry - start_p.ry) as f64;
                let width = end_p.v - start_v;
                let (a,b) = match (start_p.a, start_p.b) {
                    (Some(a), Some(b)) => (a,b),
                    _ => (0.,0.)
                };
                if (a-b).abs() > f64::EPSILON {
                    Some(start_v + do_curve(x, a, b) * width)
                }
                else {
                    Some(start_v + x * width)
                }
            }
        }
    }

    fn direction_at(&self, tick: f64) -> Option<f64> {
        match self.binary_search_by(|g| g.ry.cmp(&(tick as u32))) {
            Ok(i) =>
            //On a point
            {
                let point = self[i];
                let v = point.vf.unwrap_or(point.v);

                if let Some(next) = self.get(i + 1) {
                    Some((next.v - v) / (next.ry - point.ry) as f64)
                } else {
                    Some(0.0)
                }
            }
            Err(i) =>
            //Between points
            {
                if i == 0 || i >= self.len() {
                    //Before the first point or after the last point
                    Some(0.0)
                } else {
                    let start_p = self.get(i - 1).unwrap();
                    let end_p = self.get(i).unwrap();
                    assert!(start_p.ry < end_p.ry);
                    let start_v = match start_p.vf {
                        Some(v) => v,
                        None => start_p.v,
                    };

                    Some((end_p.v - start_v) / (end_p.ry - start_p.ry) as f64)
                }
            }
        }
    }
}

impl Graph<Option<f64>> for LaserSection {
    fn value_at(&self, tick: f64) -> Option<f64> {
        let r_tick = tick - self.0 as f64;
        self.1.value_at(r_tick)
    }

    fn direction_at(&self, tick: f64) -> Option<f64> {
        let r_tick = tick - self.0 as f64;
        self.1.direction_at(r_tick)
    }
}

impl Graph<Option<f64>> for Vec<LaserSection> {
    fn value_at(&self, tick: f64) -> Option<f64> {
        match self.binary_search_by(|s| s.0.cmp(&(tick as u32))) {
            Ok(i) => self.get(i).unwrap().value_at(tick),
            Err(i) => {
                if i > 0 {
                    self.get(i - 1).unwrap().value_at(tick)
                } else {
                    None
                }
            }
        }
    }

    fn direction_at(&self, tick: f64) -> Option<f64> {
        match self.binary_search_by(|s| s.0.cmp(&(tick as u32))) {
            Ok(i) => self.get(i).unwrap().value_at(tick),
            Err(i) => {
                if i > 0 {
                    self.get(i - 1).unwrap().direction_at(tick)
                } else {
                    None
                }
            }
        }
    }
}
