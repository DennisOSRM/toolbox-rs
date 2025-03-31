use criterion::{Criterion, black_box, criterion_group};
use std::collections::BTreeMap;
use toolbox_rs::{tabulation_hash::TabulationHashTable, tiny_table::TinyTable};

fn insert_sequential(c: &mut Criterion) {
    let mut group = c.benchmark_group("Insert Sequential");

    group.bench_function("TabulationHashTable", |b| {
        b.iter(|| {
            let mut table = TabulationHashTable::<u32, u32>::new();
            for i in 0..1000 {
                *table.get_mut(black_box(i)) = i;
            }
        })
    });

    group.bench_function("BTreeMap", |b| {
        b.iter(|| {
            let mut map = BTreeMap::new();
            for i in 0..1000 {
                map.insert(black_box(i), i);
            }
        })
    });

    group.bench_function("TinyTable", |b| {
        b.iter(|| {
            let mut table = TinyTable::new();
            for i in 0..1000 {
                table.insert(black_box(i), i);
            }
        })
    });

    group.finish();
}

fn lookup_random(c: &mut Criterion) {
    let mut group = c.benchmark_group("Lookup Random");

    // Prepare data structures
    let mut table = TabulationHashTable::<u32, u32>::new();
    let mut map = BTreeMap::new();
    let mut tiny = TinyTable::new();

    for i in 0..1000 {
        *table.get_mut(i) = i;
        map.insert(i, i);
        tiny.insert(i, i);
    }

    group.bench_function("TabulationHashTable", |b| {
        b.iter(|| {
            for i in (0..1000).rev() {
                black_box(table.peek_value(black_box(i)));
            }
        })
    });

    group.bench_function("BTreeMap", |b| {
        b.iter(|| {
            for i in (0..1000).rev() {
                black_box(map.get(&black_box(i)));
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

    group.finish();
}

criterion_group!(tables, insert_sequential, lookup_random);
