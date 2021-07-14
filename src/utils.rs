pub trait Overlaps {
    fn overlaps(&self, other: &Self) -> bool;
    fn contains(&self, y: u32) -> bool;
}

impl Overlaps for kson::Interval {
    fn overlaps(&self, other: &Self) -> bool {
        self.y <= other.y + other.l && other.y <= self.y + self.l
    }

    fn contains(&self, y: u32) -> bool {
        (self.y..=self.y + self.l).contains(&y)
    }
}

impl Overlaps for kson::LaserSection {
    fn overlaps(&self, other: &Self) -> bool {
        match (self.v.last(), other.v.last()) {
            (Some(self_last), Some(other_last)) => {
                self.y <= other.y + other_last.ry && other.y <= self.y + self_last.ry
            }
            _ => false,
        }
    }

    fn contains(&self, y: u32) -> bool {
        if let Some(last) = self.v.last() {
            (self.y..=self.y + last.ry).contains(&y)
        } else {
            false
        }
    }
}
