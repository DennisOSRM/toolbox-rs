use rand::{distributions::Standard, Rng};
use toolbox_rs::rdx_sort::radix::Sort;

fn main() {
    let rng = rand::thread_rng();
    let mut input: Vec<f64> = rng.sample_iter(Standard).take(100_000).collect();

    let is_sorted = input.windows(2).all(|i| i[0] < i[1]);
    println!("before, is_sorted={is_sorted}");

    input.rdx_sort();

    let is_sorted = input.windows(2).all(|i| i[0] < i[1]);
    println!("after, is_sorted={is_sorted}");
}
