use rand::Rng;

pub fn weighted_random<T>(items: Vec<(T, u32)>) -> T {
    let total_weight: u32 = items.iter().map(|(_, weight)| weight).sum();
    let random = rand::thread_rng().gen_range(0..total_weight);
    let mut current_weight = 0;
    for (item, weight) in items {
        current_weight += weight;
        if current_weight > random {
            return item;
        }
    }
    unreachable!()
}
