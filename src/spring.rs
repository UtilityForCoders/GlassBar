//! Física de resorte amortiguado para animaciones fluidas estilo líquido (HyperOS / iOS).
//! Mantiene la CPU en 0% una vez que la animación se asienta.

#[derive(Clone, Copy, Debug)]
pub struct Spring {
    pub value: f32,
    pub velocity: f32,
    pub target: f32,
    pub stiffness: f32,
    pub damping: f32,
}

impl Spring {
    /// `stiffness`: velocidad de respuesta (ej. 220.0).
    /// `damping`: amortiguación (ej. 18.0 para rebote suave).
    pub fn new(value: f32, stiffness: f32, damping: f32) -> Self {
        Self {
            value,
            velocity: 0.0,
            target: value,
            stiffness,
            damping,
        }
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    /// Salta instantáneamente al valor sin animación.
    #[allow(dead_code)]
    pub fn reset(&mut self, value: f32) {
        self.value = value;
        self.target = value;
        self.velocity = 0.0;
    }

    /// Avanza la simulación `dt` segundos. Retorna `true` mientras siga en movimiento.
    pub fn step(&mut self, dt: f32) -> bool {
        let mut remaining = dt.min(0.1);
        while remaining > 0.0 {
            let h = remaining.min(1.0 / 240.0);
            let force = self.stiffness * (self.target - self.value) - self.damping * self.velocity;
            self.velocity += force * h;
            self.value += self.velocity * h;
            remaining -= h;
        }

        let settled = (self.target - self.value).abs() < 0.0005 && self.velocity.abs() < 0.005;
        if settled {
            self.value = self.target;
            self.velocity = 0.0;
        }
        !settled
    }
}
