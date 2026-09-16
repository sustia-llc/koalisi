//! K7 gauntlet — the harness skeleton exercised end to end: the default
//! instance spec, one smoke arm over seeds `0..3`, the per-seed table and the
//! summary. Zero registrations.
//!
//! Run: `cargo run --features harness --example gauntlet`.

use koalisi::algorithms::SynergisticCalculator;
use koalisi::decision::ThresholdPolicy;
use koalisi::harness::{
    Arm, InstanceSpec, SeedRange, print_per_seed_table, print_summary, run_battery,
};

fn main() {
    println!("# K7 gauntlet — harness skeleton, zero registrations");
    println!();

    let spec = InstanceSpec::default();
    println!("## default InstanceSpec");
    println!();
    println!("- universe_bits: {}", spec.universe_bits);
    println!("- pool: {:?}", spec.pool);
    println!("- caps_per_agent: {:?}", spec.caps_per_agent);
    println!("- trust: {:?}", spec.trust);
    println!("- tasks: {}", spec.tasks);
    println!("- required_bits: {:?}", spec.required_bits);
    println!();

    let seeds = SeedRange { start: 0, end: 3 };
    let arm = Arm {
        label: "smoke (not a registration)".to_string(),
        policy: Box::new(ThresholdPolicy::new(SynergisticCalculator, 0.0, 0.0)),
    };
    println!("## smoke arm over seeds {seeds}");
    println!();

    let result = run_battery(&arm, &spec, seeds);
    print_per_seed_table(&result);
    print_summary(std::slice::from_ref(&result));
}
