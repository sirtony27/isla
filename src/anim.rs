/// Damped spring used for every animated value (size, radius, opacity).
/// Springs keep their velocity when the target changes mid-flight, so
/// interrupted transitions stay smooth instead of restarting.
#[derive(Clone, Copy)]
pub struct Spring {
    pub value: f32,
    pub target: f32,
    velocity: f32,
    omega: f32,
    zeta: f32,
    epsilon: f32,
}

impl Spring {
    pub const fn new(value: f32, omega: f32, zeta: f32, epsilon: f32) -> Self {
        Self {
            value,
            target: value,
            velocity: 0.0,
            omega,
            zeta,
            epsilon,
        }
    }

    pub fn set(&mut self, target: f32) {
        self.target = target;
    }

    pub fn step(&mut self, dt: f32) {
        // Semi-implicit Euler in small sub-steps: stable for stiff springs at any frame rate.
        let steps = (dt / (1.0 / 240.0)).ceil().max(1.0);
        let h = dt / steps;
        for _ in 0..steps as u32 {
            let accel = -self.omega * self.omega * (self.value - self.target)
                - 2.0 * self.zeta * self.omega * self.velocity;
            self.velocity += accel * h;
            self.value += self.velocity * h;
        }
    }

    /// Returns true (and snaps to the target) once the motion is imperceptible.
    pub fn settle(&mut self) -> bool {
        if (self.value - self.target).abs() < self.epsilon
            && self.velocity.abs() < self.epsilon * 10.0
        {
            self.value = self.target;
            self.velocity = 0.0;
            true
        } else {
            false
        }
    }
}

/// Spring for sizes in DIPs: quick with a hint of overshoot.
pub const fn size(value: f32) -> Spring {
    Spring::new(value, 22.0, 0.78, 0.05)
}

/// Spring for opacities in 0..=1: fast and without overshoot.
pub const fn fade(value: f32) -> Spring {
    Spring::new(value, 26.0, 1.0, 0.002)
}
