use std::collections::HashMap;
use std::time::Duration;

use rand::seq::SliceRandom;

use crate::error::GatewayError;
use crate::pool::{Endpoint, ExecutionTarget, PoolId, ProviderPool};
use crate::retry::{LoadBalancingStrategy, RetryPolicy};

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct ExecutionSnapshot {
    pub pool_id: Option<PoolId>,
    pub endpoints: Vec<Endpoint>,
    pub load_balancing: LoadBalancingStrategy,
    pub retry_policy: RetryPolicy,
    pub metadata: HashMap<String, String>,
    selection_key: String,
}

impl ExecutionSnapshot {
    pub(crate) fn ordered_endpoints(
        &self,
        rr_counters: &mut HashMap<String, usize>,
        max_attempts: usize,
    ) -> Result<Vec<Endpoint>, GatewayError> {
        if self.endpoints.is_empty() {
            return Err(GatewayError::NoAvailableEndpoint);
        }

        let mut endpoints = self.endpoints.clone();
        match self.load_balancing {
            LoadBalancingStrategy::Fallback => {}
            LoadBalancingStrategy::Random => {
                let mut rng = rand::thread_rng();
                endpoints.shuffle(&mut rng);
            }
            LoadBalancingStrategy::RoundRobin => {
                let idx = rr_counters.entry(self.selection_key.clone()).or_insert(0);
                let start = *idx % endpoints.len();
                endpoints.rotate_left(start);
                *idx = (*idx + 1) % endpoints.len();
            }
        }

        endpoints.truncate(max_attempts.max(1).min(endpoints.len()));
        Ok(endpoints)
    }

    #[cfg(test)]
    pub(crate) fn select_endpoint(
        &self,
        rr_counters: &mut HashMap<String, usize>,
    ) -> Result<Endpoint, GatewayError> {
        self.ordered_endpoints(rr_counters, 1)
            .map(|mut endpoints| endpoints.remove(0))
    }
}

pub(crate) fn build_execution_snapshot(
    pools: &HashMap<PoolId, ProviderPool>,
    target: &ExecutionTarget,
    default_retry_policy: &RetryPolicy,
    default_timeout: Option<Duration>,
) -> Result<ExecutionSnapshot, GatewayError> {
    match target {
        ExecutionTarget::Pool { pool_id } => {
            let pool = pools
                .get(pool_id)
                .ok_or_else(|| GatewayError::PoolNotFound(pool_id.clone()))?;
            let endpoints = enabled_endpoints(&pool.endpoints);
            if endpoints.is_empty() {
                return Err(GatewayError::NoAvailableEndpoint);
            }

            Ok(ExecutionSnapshot {
                pool_id: Some(pool.pool_id.clone()),
                endpoints,
                load_balancing: pool.load_balancing.clone(),
                retry_policy: effective_retry_policy(
                    &pool.retry_policy,
                    default_retry_policy,
                    default_timeout,
                ),
                metadata: pool.metadata.clone(),
                selection_key: format!("pool:{}", pool.pool_id),
            })
        }
        ExecutionTarget::Plan(plan) => {
            build_plan_snapshot(pools, plan, default_retry_policy, default_timeout)
        }
    }
}

fn build_plan_snapshot(
    pools: &HashMap<PoolId, ProviderPool>,
    plan: &crate::pool::ExecutionPlan,
    default_retry_policy: &RetryPolicy,
    default_timeout: Option<Duration>,
) -> Result<ExecutionSnapshot, GatewayError> {
    let (pool_id, endpoints, inherited_strategy, inherited_retry) = if let Some(pool_id) =
        &plan.pool_id
    {
        let pool = pools
            .get(pool_id)
            .ok_or_else(|| GatewayError::PoolNotFound(pool_id.clone()))?;
        let endpoints = plan
            .candidates
            .iter()
            .map(|candidate| {
                pool.endpoints
                    .iter()
                    .find(|endpoint| {
                        endpoint.enabled && endpoint.endpoint_id == candidate.endpoint_id
                    })
                    .cloned()
                    .ok_or_else(|| GatewayError::EndpointNotFound(candidate.endpoint_id.clone()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        (
            Some(pool.pool_id.clone()),
            endpoints,
            pool.load_balancing.clone(),
            effective_retry_policy(&pool.retry_policy, default_retry_policy, default_timeout),
        )
    } else {
        let endpoints = resolve_global_plan_endpoints(pools, &plan.candidates)?;
        (
            None,
            endpoints,
            LoadBalancingStrategy::RoundRobin,
            effective_retry_policy(
                &RetryPolicy::default(),
                default_retry_policy,
                default_timeout,
            ),
        )
    };

    if endpoints.is_empty() {
        return Err(GatewayError::NoAvailableEndpoint);
    }

    let load_balancing = plan
        .load_balancing_override
        .clone()
        .unwrap_or(inherited_strategy);
    let retry_policy = plan
        .retry_policy_override
        .clone()
        .map(|policy| effective_retry_policy(&policy, default_retry_policy, default_timeout))
        .unwrap_or(inherited_retry);

    let selection_key = if let Some(pool_id) = &pool_id {
        format!(
            "plan:{}:{}",
            pool_id,
            plan.candidates
                .iter()
                .map(|candidate| candidate.endpoint_id.as_str())
                .collect::<Vec<_>>()
                .join(",")
        )
    } else {
        format!(
            "plan:global:{}",
            plan.candidates
                .iter()
                .map(|candidate| candidate.endpoint_id.as_str())
                .collect::<Vec<_>>()
                .join(",")
        )
    };

    Ok(ExecutionSnapshot {
        pool_id,
        endpoints,
        load_balancing,
        retry_policy,
        metadata: plan.metadata.clone(),
        selection_key,
    })
}

fn resolve_global_plan_endpoints(
    pools: &HashMap<PoolId, ProviderPool>,
    candidates: &[crate::pool::EndpointRef],
) -> Result<Vec<Endpoint>, GatewayError> {
    let all_endpoints: Vec<&Endpoint> = pools
        .values()
        .flat_map(|pool| pool.endpoints.iter())
        .filter(|endpoint| endpoint.enabled)
        .collect();

    candidates
        .iter()
        .map(|candidate| {
            let matches: Vec<&Endpoint> = all_endpoints
                .iter()
                .copied()
                .filter(|endpoint| endpoint.endpoint_id == candidate.endpoint_id)
                .collect();

            match matches.len() {
                0 => Err(GatewayError::EndpointNotFound(
                    candidate.endpoint_id.clone(),
                )),
                1 => Ok(matches[0].clone()),
                _ => Err(GatewayError::InvalidRequest(format!(
                    "ambiguous endpoint_id in execution plan: {}",
                    candidate.endpoint_id
                ))),
            }
        })
        .collect()
}

fn enabled_endpoints(endpoints: &[Endpoint]) -> Vec<Endpoint> {
    endpoints
        .iter()
        .filter(|endpoint| endpoint.enabled)
        .cloned()
        .collect()
}

fn effective_retry_policy(
    policy: &RetryPolicy,
    default_retry_policy: &RetryPolicy,
    default_timeout: Option<Duration>,
) -> RetryPolicy {
    let mut effective = if *policy == RetryPolicy::default() {
        default_retry_policy.clone()
    } else {
        policy.clone()
    };

    if effective.per_attempt_timeout.is_none() {
        effective.per_attempt_timeout = default_timeout;
    }

    effective
}
