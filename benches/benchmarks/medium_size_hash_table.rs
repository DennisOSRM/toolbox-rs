use criterion::{Criterion, black_box, criterion_group};
use std::collections::{BTreeMap, HashMap};
use toolbox_rs::{
    fibonacci_hash::FibonacciHash, medium_size_hash_table::MediumSizeHashTable,
    tabulation_hash::TabulationHash, tiny_table::TinyTable,
};

fn insert_sequential(c: &mut Criterion) {
    let mut group = c.benchmark_group("Insert Sequential");

    // Prepare data structures
    let mut table = MediumSizeHashTable::<u32, u32, TabulationHash>::new();
    let mut fibonacci = MediumSizeHashTable::<u32, u32, FibonacciHash>::new();
    let mut bmap = BTreeMap::new();
    let mut tiny = TinyTable::new();
    let mut hashmap = HashMap::new();

    group.bench_function("TabulationHashTable", |b| {
        b.iter(|| {
            for i in 0..1000 {
                *table.get_mut(black_box(i)) = i;
            }
        })
    });

    group.bench_function("FibonacciHashTable", |b| {
        b.iter(|| {
            for i in 0..1000 {
                *fibonacci.get_mut(black_box(i)) = i;
            }
        })
    });

    group.bench_function("BTreeMap", |b| {
        b.iter(|| {
            for i in 0..1000 {
                bmap.insert(black_box(i), i);
            }
        })
    });

    group.bench_function("TinyTable", |b| {
        b.iter(|| {
            for i in 0..1000 {
                tiny.insert(black_box(i), i);
            }
        })
    });

    group.bench_function("Hashmap", |b| {
        b.iter(|| {
            for i in 0..1000 {
                hashmap.insert(black_box(i), i);
            }
        })
    });

    group.finish();
}

fn lookup_random(c: &mut Criterion) {
    let mut group = c.benchmark_group("Lookup Random");

    // Prepare data structures
    let mut table = MediumSizeHashTable::<u32, u32, TabulationHash>::new();
    let mut fibonacci = MediumSizeHashTable::<u32, u32, FibonacciHash>::new();
    let mut bmap = BTreeMap::new();
    let mut tiny = TinyTable::new();
    let mut hashmap = HashMap::new();

    for i in 0..1000 {
        *fibonacci.get_mut(i) = i;
        *table.get_mut(i) = i;
        bmap.insert(i, i);
        tiny.insert(i, i);
        hashmap.insert(i, i);
    }

    group.bench_function("TabulationHashTable", |b| {
        b.iter(|| {
            for i in (0..1000).rev() {
                black_box(table.peek_value(black_box(i)));
            }
        })
    });

    group.bench_function("FibonacciHashTable", |b| {
        b.iter(|| {
            for i in (0..1000).rev() {
                black_box(fibonacci.peek_value(black_box(i)));
            }
        })
    });

    group.bench_function("BTreeMap", |b| {
        b.iter(|| {
            for i in (0..1000).rev() {
                black_box(bmap.get(&black_box(i)));
            }
        })
    });

    group.bench_function("TinyTable", |b| {
        b.iter(|| {
            for i in (0..1000).rev() {
                black_box(tiny.find(black_box(&i)));
            }
        })
    });

    group.bench_function("Hashmap", |b| {
        b.iter(|| {
            for i in (0..1000).rev() {
                black_box(hashmap.get(black_box(&i)));
            }
        })
    });

    group.finish();
}

criterion_group!(tables, insert_sequential, lookup_random);
