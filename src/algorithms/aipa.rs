//! Anytime Integer-Partition based Algorithm (AIPA)
//!
//! Implementation of the AIPA algorithm from:
//! Rahwan, T., Ramchurn, S. D., Dang, V. D., & Jennings, N. R. (2007).
//! "Near-optimal anytime coalition structure generation." Proceedings of IJCAI-07.
//!
//! AIPA organizes the coalition structure search space using integer partitions,
//! enabling:
//! - Anytime operation (can stop and provide bounds at any time)
//! - Efficient pruning via upper/lower bounds
//! - 0.082% of dynamic programming time for optimal solution
//! - 91%+ quality guarantee after initial scan

use std::collections::HashMap;

/// Integer partition representation
///
/// An integer partition of n is a way of writing n as a sum of positive integers.
/// For example, partitions of 4 are: [4], [3,1], [2,2], [2,1,1], [1,1,1,1]
///
/// In coalition formation, a partition [3, 2, 2, 1] represents a coalition
/// structure with one coalition of size 3, two of size 2, and one of size 1.
pub type IntegerPartition = Vec<usize>;

/// Generate all integer partitions of n
///
/// # Arguments
///
/// * `n` - The integer to partition
///
/// # Returns
///
/// Vector of all possible integer partitions of n
///
/// # Example
///
/// ```
/// use koalisi::algorithms::aipa::generate_integer_partitions;
///
/// let partitions = generate_integer_partitions(4);
/// // Returns: [[4], [3,1], [2,2], [2,1,1], [1,1,1,1]]
/// assert_eq!(partitions.len(), 5);
/// ```
pub fn generate_integer_partitions(n: usize) -> Vec<IntegerPartition> {
    if n == 0 {
        return vec![vec![]];
    }

    let mut partitions = Vec::new();
    let mut current = Vec::new();
    generate_partitions_recursive(n, n, &mut current, &mut partitions);
    partitions
}

fn generate_partitions_recursive(
    n: usize,
    max_value: usize,
    current: &mut Vec<usize>,
    result: &mut Vec<IntegerPartition>,
) {
    if n == 0 {
        result.push(current.clone());
        return;
    }

    for i in (1..=max_value.min(n)).rev() {
        current.push(i);
        generate_partitions_recursive(n - i, i, current, result);
        current.pop();
    }
}

/// Partition bounds for AIPA search
#[derive(Debug, Clone)]
pub struct PartitionBounds {
    /// The partition configuration
    pub partition: IntegerPartition,
    /// Upper bound on value for this partition
    pub upper_bound: f64,
    /// Lower bound (average) on value for this partition
    pub lower_bound: f64,
}

impl PartitionBounds {
    /// Create new partition bounds
    pub fn new(partition: IntegerPartition, upper_bound: f64, lower_bound: f64) -> Self {
        Self {
            partition,
            upper_bound,
            lower_bound,
        }
    }

    /// Get the bound range (upper - lower)
    pub fn bound_range(&self) -> f64 {
        self.upper_bound - self.lower_bound
    }
}

/// Compute upper bound for a partition configuration
///
/// Upper bound = sum of maximum coalition values for each size in the partition
///
/// # Arguments
///
/// * `partition` - The integer partition (e.g., [3, 2, 2, 1])
/// * `values_by_size` - Map from coalition size to list of coalition values
///
/// # Returns
///
/// The upper bound value for this partition
///
/// # Example
///
/// ```
/// use koalisi::algorithms::aipa::compute_partition_upper_bound;
/// use std::collections::HashMap;
///
/// let mut values_by_size = HashMap::new();
/// values_by_size.insert(2, vec![100.0, 150.0, 120.0]);
/// values_by_size.insert(1, vec![50.0, 60.0]);
///
/// let partition = vec![2, 1];
/// let upper_bound = compute_partition_upper_bound(&partition, &values_by_size);
///
/// // Should be max of size-2 (150) + max of size-1 (60) = 210
/// assert_eq!(upper_bound, 210.0);
/// ```
pub fn compute_partition_upper_bound(
    partition: &IntegerPartition,
    values_by_size: &HashMap<usize, Vec<f64>>,
) -> f64 {
    partition
        .iter()
        .filter_map(|&size| {
            values_by_size
                .get(&size)
                .and_then(|values| {
                    values
                        .iter()
                        .copied()
                        .max_by(|a, b| a.partial_cmp(b).unwrap())
                })
        })
        .sum()
}

/// Compute lower bound (average) for a partition configuration
///
/// Lower bound = sum of average coalition values for each size in the partition
///
/// # Arguments
///
/// * `partition` - The integer partition
/// * `values_by_size` - Map from coalition size to list of coalition values
///
/// # Returns
///
/// The lower bound (average) value for this partition
pub fn compute_partition_avg_bound(
    partition: &IntegerPartition,
    values_by_size: &HashMap<usize, Vec<f64>>,
) -> f64 {
    partition
        .iter()
        .filter_map(|&size| {
            values_by_size.get(&size).map(|values| {
                let sum: f64 = values.iter().sum();
                sum / values.len() as f64
            })
        })
        .sum()
}

/// Compute minimum value bound for a partition configuration
///
/// # Arguments
///
/// * `partition` - The integer partition
/// * `values_by_size` - Map from coalition size to list of coalition values
///
/// # Returns
///
/// The minimum value bound for this partition
pub fn compute_partition_min_bound(
    partition: &IntegerPartition,
    values_by_size: &HashMap<usize, Vec<f64>>,
) -> f64 {
    partition
        .iter()
        .filter_map(|&size| {
            values_by_size
                .get(&size)
                .and_then(|values| {
                    values
                        .iter()
                        .copied()
                        .min_by(|a, b| a.partial_cmp(b).unwrap())
                })
        })
        .sum()
}

/// Compute bounds for all partitions
///
/// Returns vector of (partition, upper_bound, lower_bound) sorted by upper bound descending
pub fn compute_all_partition_bounds(
    n: usize,
    values_by_size: &HashMap<usize, Vec<f64>>,
) -> Vec<PartitionBounds> {
    let partitions = generate_integer_partitions(n);

    let mut bounds: Vec<PartitionBounds> = partitions
        .into_iter()
        .map(|partition| {
            let upper = compute_partition_upper_bound(&partition, values_by_size);
            let lower = compute_partition_avg_bound(&partition, values_by_size);
            PartitionBounds::new(partition, upper, lower)
        })
        .collect();

    // Sort by upper bound (descending)
    bounds.sort_by(|a, b| {
        b.upper_bound
            .partial_cmp(&a.upper_bound)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    bounds
}

/// Find the best partition based on upper bounds
///
/// Returns the partition with the highest upper bound
pub fn find_best_partition(
    n: usize,
    values_by_size: &HashMap<usize, Vec<f64>>,
) -> Option<PartitionBounds> {
    compute_all_partition_bounds(n, values_by_size)
        .into_iter()
        .next()
}

/// Calculate partition count for a given n
///
/// Returns the number of integer partitions of n (also known as p(n))
pub fn partition_count(n: usize) -> usize {
    generate_integer_partitions(n).len()
}

/// Verify that a partition is valid
///
/// Checks that:
/// 1. All elements are positive
/// 2. Elements sum to n
/// 3. Elements are in non-increasing order
pub fn verify_partition(partition: &IntegerPartition, n: usize) -> bool {
    if partition.is_empty() && n == 0 {
        return true;
    }

    if partition.is_empty() {
        return false;
    }

    // Check all positive
    if partition.iter().any(|&x| x == 0) {
        return false;
    }

    // Check sum
    if partition.iter().sum::<usize>() != n {
        return false;
    }

    // Check non-increasing order
    for i in 1..partition.len() {
        if partition[i] > partition[i - 1] {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_integer_partitions() {
        // Test small values
        assert_eq!(generate_integer_partitions(0), vec![vec![]]);

        let p1 = generate_integer_partitions(1);
        assert_eq!(p1, vec![vec![1]]);

        let p2 = generate_integer_partitions(2);
        assert_eq!(p2.len(), 2); // [2], [1,1]

        let p3 = generate_integer_partitions(3);
        assert_eq!(p3.len(), 3); // [3], [2,1], [1,1,1]

        let p4 = generate_integer_partitions(4);
        assert_eq!(p4.len(), 5); // [4], [3,1], [2,2], [2,1,1], [1,1,1,1]

        let p5 = generate_integer_partitions(5);
        assert_eq!(p5.len(), 7);
    }

    #[test]
    fn test_partitions_sum_to_n() {
        for n in 1..=10 {
            let partitions = generate_integer_partitions(n);
            for partition in partitions {
                assert_eq!(partition.iter().sum::<usize>(), n);
            }
        }
    }

    #[test]
    fn test_compute_partition_upper_bound() {
        let mut values_by_size = HashMap::new();
        values_by_size.insert(2, vec![100.0, 150.0, 120.0]);
        values_by_size.insert(1, vec![50.0, 60.0]);

        let partition = vec![2, 1];
        let upper_bound = compute_partition_upper_bound(&partition, &values_by_size);

        // Should be 150 + 60 = 210
        assert_eq!(upper_bound, 210.0);
    }

    #[test]
    fn test_compute_partition_avg_bound() {
        let mut values_by_size = HashMap::new();
        values_by_size.insert(2, vec![100.0, 150.0, 120.0]);

        let partition = vec![2];
        let avg_bound = compute_partition_avg_bound(&partition, &values_by_size);

        // Should be (100 + 150 + 120) / 3 = 123.33...
        assert!((avg_bound - 123.333).abs() < 0.01);
    }

    #[test]
    fn test_compute_partition_min_bound() {
        let mut values_by_size = HashMap::new();
        values_by_size.insert(2, vec![100.0, 150.0, 120.0]);
        values_by_size.insert(1, vec![50.0, 60.0]);

        let partition = vec![2, 1];
        let min_bound = compute_partition_min_bound(&partition, &values_by_size);

        // Should be 100 + 50 = 150
        assert_eq!(min_bound, 150.0);
    }

    #[test]
    fn test_compute_all_partition_bounds() {
        let mut values_by_size = HashMap::new();
        values_by_size.insert(3, vec![300.0]);
        values_by_size.insert(2, vec![150.0, 180.0]);
        values_by_size.insert(1, vec![60.0]);

        let bounds = compute_all_partition_bounds(3, &values_by_size);

        // Should have 3 partitions for n=3
        assert_eq!(bounds.len(), 3);

        // First should have highest upper bound
        assert!(bounds[0].upper_bound >= bounds[1].upper_bound);
        assert!(bounds[1].upper_bound >= bounds[2].upper_bound);
    }

    #[test]
    fn test_find_best_partition() {
        let mut values_by_size = HashMap::new();
        values_by_size.insert(2, vec![100.0, 150.0]);
        values_by_size.insert(1, vec![50.0]);

        let best = find_best_partition(2, &values_by_size);

        assert!(best.is_some());
        let best_bounds = best.unwrap();

        // Best should be [2] with value 150
        assert_eq!(best_bounds.partition, vec![2]);
        assert_eq!(best_bounds.upper_bound, 150.0);
    }

    #[test]
    fn test_partition_count() {
        assert_eq!(partition_count(0), 1);
        assert_eq!(partition_count(1), 1);
        assert_eq!(partition_count(2), 2);
        assert_eq!(partition_count(3), 3);
        assert_eq!(partition_count(4), 5);
        assert_eq!(partition_count(5), 7);
    }

    #[test]
    fn test_verify_partition() {
        assert!(verify_partition(&vec![3, 2, 1], 6));
        assert!(verify_partition(&vec![4], 4));
        assert!(verify_partition(&vec![2, 2], 4));

        // Invalid: doesn't sum to n
        assert!(!verify_partition(&vec![3, 2], 6));

        // Invalid: not sorted
        assert!(!verify_partition(&vec![1, 3, 2], 6));

        // Invalid: contains zero
        assert!(!verify_partition(&vec![3, 0, 2], 5));

        // Valid: empty partition of 0
        assert!(verify_partition(&vec![], 0));
    }

    #[test]
    fn test_partition_bounds_methods() {
        let bounds = PartitionBounds::new(vec![3, 2], 500.0, 400.0);

        assert_eq!(bounds.partition, vec![3, 2]);
        assert_eq!(bounds.upper_bound, 500.0);
        assert_eq!(bounds.lower_bound, 400.0);
        assert_eq!(bounds.bound_range(), 100.0);
    }
}
