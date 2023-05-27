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
        match (self.last(), other.last()) {
            (Some(self_last), Some(other_last)) => {
                self.tick() <= other.tick() + other_last.ry
                    && other.tick() <= self.tick() + self_last.ry
            }
            _ => false,
        }
    }

    fn contains(&self, y: u32) -> bool {
        if let Some(last) = self.last() {
            (self.tick()..=self.tick() + last.ry).contains(&y)
        } else {
            false
        }
    }
}
