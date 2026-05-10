/*
This is part of WHY2
Copyright (C) 2022-2026 Václav Šmejkal

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/

use std::hint::black_box;

use rand::RngExt;

use criterion::
{
    criterion_group,
    criterion_main,
    Criterion,
    Throughput,
    BenchmarkId,
};

use why2::
{
    encrypter,
    decrypter,
    crypto,
    grid::Grid,
};

//GRID SIZE
const W: usize = 8;
const H: usize = 8;

//MICRO-BENCHMARKS
fn bench_grid_internals(c: &mut Criterion)
{
    let mut group = c.benchmark_group("Grid Internals");

    //DATA PREP
    let mut rng = rand::rng();
    let key_data: Vec<i64> = (0..(W * H * 2)).map(|_| rng.random()).collect();
    let mut grid = Grid::<W, H>::new().unwrap();
    let key_grid = Grid::<W, H>::from_key(&key_data).unwrap();

    //SUBCELL
    group.bench_function("Subcell (1 Round)", |b|
    {
        b.iter(||
        {
            black_box(&mut grid).subcell(black_box(0));
        })
    });

    //MIX COLUMNS
    group.bench_function("MixColumns", |b|
    {
        b.iter(||
        {
            black_box(&mut grid).mix_columns();
        })
    });

    //SHIFT ROWS
    let shifts = key_grid.precalculate_shifts();
    group.bench_function("ShiftRows", |b|
    {
        b.iter(||
        {
            black_box(&mut grid).shift_rows(black_box(&shifts));
        })
    });

    group.finish();
}

//LATENCY
fn bench_latency(c: &mut Criterion)
{
    let mut group = c.benchmark_group("Latency");

    //KEYGEN
    group.bench_function("Key Generation", |b|
    {
        b.iter(||
        {
            crypto::generate_key::<W, H>()
        })
    });

    //KEY SCHEDULE
    let mut rng = rand::rng();
    let key_data: Vec<i64> = (0..(W * H * 2)).map(|_| rng.random()).collect();
    let key_grid = Grid::<W, H>::from_key(&key_data).unwrap();

    group.bench_function("Key Schedule (Round Keys Gen)", |b|
    {
        b.iter(||
        {
            crypto::generate_round_keys(black_box(&key_grid))
        })
    });

    group.finish();
}

//THROUGHPUT
fn bench_throughput(c: &mut Criterion)
{
    let mut group = c.benchmark_group("Full Pipeline Throughput");

    //128B, 64KB, 1MB
    let sizes = [128, 64 * 1024, 1024 * 1024];

    for size in sizes.iter()
    {
        let element_count = size / 8;
        let mut rng = rand::rng();
        let input: Vec<i64> = (0..element_count).map(|_| rng.random()).collect();

        //SET METRIC TO MB/s
        group.throughput(Throughput::Bytes(*size as u64));

        //ENCRYPTION MEASSURE
        group.bench_with_input(BenchmarkId::new("Encrypt", size), size, |b, &_s|
        {
            b.iter(||
            {
                encrypter::encrypt::<W, H>(black_box(&input), None)
            })
        });

        //DECRYPTION PREP
        let encrypted = encrypter::encrypt::<W, H>(&input, None).unwrap();

        //DECRYPTION MEASSURE
        group.bench_with_input(BenchmarkId::new("Decrypt", size), size, |b, &_s|
        {
            b.iter_with_large_drop(||
            {
                decrypter::decrypt::<W, H>(black_box(encrypted.clone()))
            })
        });
    }

    group.finish();
}

criterion_group!(benches, bench_grid_internals, bench_latency, bench_throughput);
criterion_main!(benches);
