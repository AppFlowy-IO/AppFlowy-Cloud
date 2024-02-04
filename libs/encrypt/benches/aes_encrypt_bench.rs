use criterion::{black_box, criterion_group, criterion_main, Criterion};
use encrypt::aes_encrypt::{decrypt_data, encrypt_data};

fn encryption_benchmark(c: &mut Criterion) {
  let shared_secret =
    hex::decode("cc66c018bfe0a7af8ce0f98847d2ead96a9927df16111068bf98a79f40b39e00").unwrap();

  let data = "Hello world!".as_bytes().to_vec();

  c.bench_function("encrypt_data", |b| {
    b.iter(|| encrypt_data(black_box(&data), black_box(&shared_secret)))
  });
}

fn decryption_benchmark(c: &mut Criterion) {
  let shared_secret =
    hex::decode("cc66c018bfe0a7af8ce0f98847d2ead96a9927df16111068bf98a79f40b39e00").unwrap();

  let data = "Hello world!".as_bytes().to_vec();
  let encrypted_data = encrypt_data(data, &shared_secret).unwrap();

  c.bench_function("decrypt_data", |b| {
    b.iter(|| decrypt_data(black_box(&encrypted_data), black_box(&shared_secret)))
  });
}

criterion_group!(benches, encryption_benchmark, decryption_benchmark);
criterion_main!(benches);
