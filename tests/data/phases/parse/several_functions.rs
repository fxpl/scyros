use std::f64::consts::PI;

// --- Trait: FloatOps (equivalent to Scala trait) ---

trait FloatOps {
    fn compute(&self, x: f64) -> f64;

    fn description(&self) -> &str {
        "Performs float operations"
    }
}

// --- Trait: AbstractFloatProcessor (equivalent to Scala abstract class) ---

trait AbstractFloatProcessor {
    fn process(&self, values: &[f64]) -> f64;
    fn name(&self) -> &str;
}

// --- Struct: ConcreteFloatProcessor ---

struct ConcreteFloatProcessor;

impl AbstractFloatProcessor for ConcreteFloatProcessor {
    fn process(&self, values: &[f64]) -> f64 {
        let mut sum = 0.0_f64;
        // Handle NaN values by treating them as zero
        for &v in values {
            sum += if v.is_nan() { 0.0 } else { v };
        }
        sum / if values.is_empty() { 1.0 } else { values.len() as f64 }
    }

    fn name(&self) -> &str {
        "ConcreteFloatProcessor"
    }
}

impl FloatOps for ConcreteFloatProcessor {
    fn compute(&self, x: f64) -> f64 {
        if x == f64::INFINITY {
            0.0
        } else if x == f64::NEG_INFINITY {
            0.0
        } else if x.is_nan() {
            -1.0
        } else if x > 0.0 {
            x.sqrt() + x.ln()
        } else if x < 0.0 {
            x.abs() * x.sin()
        } else {
            0.0
        }
    }
}

// --- Module: FloatUtils (equivalent to Scala companion object) ---

mod float_utils {
    pub fn factorial(n: u32) -> f64 {
        let mut result = 1.0_f64;
        let mut i = 1_u32;
        while i <= n {
            result *= i as f64;
            i += 1;
        }
        result
    }

    pub fn sum_until_epsilon(start: f64, epsilon: f64) -> f64 {
        let mut sum = 0.0_f64;
        let mut term = start;
        // This loop continues to add terms until the absolute value of the term
        // is less than epsilon. The term is halved each iteration,
        // simulating a converging series.
        loop {
            sum += term;
            term /= 2.0;
            if term.abs() <= epsilon {
                break;
            }
        }
        sum
    }

    pub fn find_first_negative(xs: &[f64]) -> Option<f64> {
        xs.iter().copied().find(|&x| x < 0.0)
    }

    pub fn transcendental_ops(x: f64) -> f64 {
        x.exp() + x.cos() - x.tanh()
    }

    pub fn special_values_demo() -> Vec<f64> {
        vec![
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::MIN,
            f64::MAX,
            0.0,
            -0.0,
        ]
    }
}

// --- Main ---

fn main() {
    let processor = ConcreteFloatProcessor;
    let data: Vec<f64> = vec![1.0, 2.0, f64::NAN, -3.0, f64::INFINITY];

    println!("Processed: {}", processor.process(&data));
    println!("Compute(4.0): {}", processor.compute(4.0));
    println!("Factorial(5): {}", float_utils::factorial(5));
    println!(
        "Sum until epsilon: {}",
        float_utils::sum_until_epsilon(1.0, 1e-5)
    );
    println!(
        "First negative: {:?}",
        float_utils::find_first_negative(&data)
    );
    println!(
        "Transcendental ops: {}",
        float_utils::transcendental_ops(PI)
    );

    let special: Vec<String> = float_utils::special_values_demo()
        .iter()
        .map(|v| v.to_string())
        .collect();
    println!("Special values: {}", special.join(", "));
}
