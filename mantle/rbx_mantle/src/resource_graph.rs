use std::{
    collections::{BTreeMap, HashMap},
    marker::PhantomData,
};

use async_trait::async_trait;
use difference::Changeset;
use serde::Serialize;
use yansi::Paint;

macro_rules! all_outputs {
    ($expr:expr, $enum:path) => {{
        $expr
            .iter()
            .filter_map(|value| {
                if let $enum(outputs) = value {
                    Some(outputs)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    }};
}
pub(crate) use all_outputs;

macro_rules! single_output {
    ($expr:expr, $enum:path) => {{
        *all_outputs!($expr, $enum)
            .first()
            .expect("Missing expected output")
    }};
}
pub(crate) use single_output;

macro_rules! optional_output {
    ($expr:expr, $enum:path) => {{
        all_outputs!($expr, $enum).first().map(|output| *output)
    }};
}
pub(crate) use optional_output;

pub type ResourceId = String;

pub trait Resource<TInputs, TOutputs>: Clone {
    fn get_id(&self) -> ResourceId;
    fn get_inputs_hash(&self) -> String;
    fn get_outputs_hash(&self) -> String;
    fn get_inputs(&self) -> TInputs;
    fn get_outputs(&self) -> Option<TOutputs>;
    fn get_dependencies(&self) -> Vec<ResourceId>;
    fn set_outputs(&mut self, outputs: TOutputs);
}

#[async_trait]
pub trait ResourceManager<TInputs, TOutputs> {
    async fn get_create_price(
        &self,
        inputs: TInputs,
        dependency_outputs: Vec<TOutputs>,
    ) -> Result<Option<u32>, String>;

    async fn create(
        &self,
        inputs: TInputs,
        dependency_outputs: Vec<TOutputs>,
        price: Option<u32>,
    ) -> Result<TOutputs, String>;

    async fn get_update_price(
        &self,
        inputs: TInputs,
        outputs: TOutputs,
        dependency_outputs: Vec<TOutputs>,
    ) -> Result<Option<u32>, String>;

    async fn update(
        &self,
        inputs: TInputs,
        outputs: TOutputs,
        dependency_outputs: Vec<TOutputs>,
        price: Option<u32>,
    ) -> Result<TOutputs, String>;

    async fn delete(
        &self,
        outputs: TOutputs,
        dependency_outputs: Vec<TOutputs>,
    ) -> Result<(), String>;

    /// Whether updating a resource with these inputs means deleting the existing resource and
    /// creating a new one. The graph then runs the delete and create steps itself, so it knows
    /// the old resource is gone if the create step fails.
    fn replaces_on_update(&self, _inputs: &TInputs) -> bool {
        false
    }
}

#[derive(Default, Clone)]
pub struct EvaluateResults {
    pub created_count: u32,
    pub updated_count: u32,
    pub deleted_count: u32,
    pub noop_count: u32,
    pub skipped_count: u32,
}

enum OperationResult<TOutputs> {
    Skipped(String),
    Noop,
    Failed(String),
    /// The previous resource was deleted as part of a replacement, but creating the new one failed.
    FailedAfterDelete(String),
    SucceededDelete,
    SucceededCreate(TOutputs),
    SucceededUpdate(TOutputs),
}

fn get_changeset(previous_hash: &str, new_hash: &str) -> Changeset {
    Changeset::new(previous_hash, new_hash, "\n")
}

pub struct ResourceGraph<TResource, TInputs, TOutputs>
where
    TResource: Resource<TInputs, TOutputs>,
    TInputs: Clone,
    TOutputs: Clone,
{
    phantom_inputs: std::marker::PhantomData<TInputs>,
    phantom_outputs: std::marker::PhantomData<TOutputs>,
    resources: HashMap<ResourceId, TResource>,
}

impl<TResource, TInputs, TOutputs> ResourceGraph<TResource, TInputs, TOutputs>
where
    TResource: Resource<TInputs, TOutputs>,
    TInputs: Clone,
    TOutputs: Clone,
    TOutputs: Serialize,
{
    pub fn new(resources: &[TResource]) -> Self {
        Self {
            resources: resources
                .iter()
                .map(|resource| (resource.get_id(), resource.clone()))
                .collect(),
            phantom_inputs: PhantomData,
            phantom_outputs: PhantomData,
        }
    }

    pub fn remove_resource(&mut self, resource_id: &str) -> Option<TResource> {
        self.resources.remove(resource_id)
    }

    pub fn get_outputs(&self, resource_id: &str) -> Option<TOutputs> {
        self.resources
            .get(resource_id)
            .and_then(|resource| resource.get_outputs())
    }

    fn get_dependency_graph(&self) -> BTreeMap<ResourceId, Vec<ResourceId>> {
        self.resources
            .iter()
            .map(|(id, resource)| (id.clone(), resource.get_dependencies()))
            .collect()
    }

    fn get_topological_order(&self) -> Result<Vec<ResourceId>, String> {
        let mut dependency_graph = self.get_dependency_graph();

        let mut start_nodes: Vec<ResourceId> = dependency_graph
            .iter()
            .filter_map(|(node, deps)| {
                if deps.is_empty() {
                    Some(node.clone())
                } else {
                    None
                }
            })
            .collect();

        let mut ordered: Vec<ResourceId> = Vec::new();
        while let Some(start_node) = start_nodes.pop() {
            ordered.push(start_node.clone());
            for (node, deps) in dependency_graph.iter_mut() {
                if deps.contains(&start_node) {
                    deps.retain(|dep| dep != &start_node);
                    if deps.is_empty() {
                        start_nodes.push(node.clone());
                    }
                }
            }
        }

        let has_cycles = dependency_graph.iter().any(|(_, deps)| !deps.is_empty());
        match has_cycles {
            true => Err("Cannot evaluate resource graph because it has cycles".to_owned()),
            false => Ok(ordered),
        }
    }

    pub fn get_resource_list(&self) -> Vec<TResource> {
        self.get_topological_order()
            .unwrap()
            .iter()
            .map(|id| self.resources.get(id).unwrap().clone())
            .collect()
    }

    fn get_dependency_outputs(&self, resource: &TResource) -> Option<Vec<TOutputs>> {
        let mut dependency_outputs: Vec<TOutputs> = Vec::new();
        for dependency in resource.get_dependencies() {
            let resource = self.resources.get(&dependency);
            if let Some(resource) = resource {
                if let Some(outputs) = resource.get_outputs() {
                    dependency_outputs.push(outputs);
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }
        Some(dependency_outputs)
    }

    fn get_dependency_outputs_hash(&self, dependency_outputs: Vec<TOutputs>) -> String {
        // TODO: Should we separate hashes from displays?
        let hash = serde_yaml::to_string(&dependency_outputs)
            .map_err(|e| format!("Failed to compute dependency outputs hash\n\t{}", e))
            .unwrap();
        if hash.is_empty() {
            ""
        } else {
            // We remove first 4 characters to remove "---\n", and we trim the end to remove "\n"
            hash[4..].trim_end()
        }
        .to_owned()
    }

    fn handle_operation_result(
        &mut self,
        results: &mut EvaluateResults,
        failures_count: &mut u32,
        previous_graph: &ResourceGraph<TResource, TInputs, TOutputs>,
        resource_id: &str,
        operation_result: OperationResult<TOutputs>,
    ) {
        // TODO: Improve DRY here
        match operation_result {
            OperationResult::SucceededDelete => {
                // No need to update the graph since it's already not present
                results.deleted_count += 1;
                let previous_resource = previous_graph.resources.get(resource_id).unwrap();
                logger::end_action_with_results(
                    "Succeeded with outputs:",
                    get_changeset(&previous_resource.get_outputs_hash(), ""),
                );
            }
            OperationResult::SucceededCreate(outputs) => {
                // Update the resource with the new outputs
                let resource = self.resources.get_mut(resource_id).unwrap();
                resource.set_outputs(outputs);

                results.created_count += 1;
                logger::end_action_with_results(
                    "Succeeded with outputs:",
                    get_changeset("", &resource.get_outputs_hash()),
                );
            }
            OperationResult::SucceededUpdate(outputs) => {
                // Update the resource with the new outputs
                let resource = self.resources.get_mut(resource_id).unwrap();
                resource.set_outputs(outputs);

                results.updated_count += 1;
                let previous_resource = previous_graph.resources.get(resource_id).unwrap();
                logger::end_action_with_results(
                    "Succeeded with outputs:",
                    get_changeset(
                        &previous_resource.get_outputs_hash(),
                        &resource.get_outputs_hash(),
                    ),
                );
            }
            OperationResult::Noop => {
                // There was no need to create or update the resource. We will update the resource
                // with the previous outputs
                let previous_resource = previous_graph.resources.get(resource_id).unwrap();
                let resource = self.resources.get_mut(resource_id).unwrap();
                resource.set_outputs(
                    previous_resource
                        .get_outputs()
                        .expect("Existing resource should have outputs."),
                );

                results.noop_count += 1;
            }
            OperationResult::Skipped(reason) => {
                // The resource was not evaluated. If the resource existed previously, we will copy
                // the old version into this graph. Otherwise, we will remove this resource from the
                // graph.
                self.restore_previous_resource(previous_graph, resource_id);

                results.skipped_count += 1;
                logger::end_action(format!("Skipped: {}", Paint::yellow(reason)));
            }
            OperationResult::Failed(error) => {
                // An error occurred while creating or updating the resource. If the
                // resource existed previously, we will copy the old version into this
                // graph. Otherwise, we will remove this resource from the graph.
                self.restore_previous_resource(previous_graph, resource_id);

                *failures_count += 1;
                logger::end_action(format!("Failed: {}", Paint::red(error)));
            }
            OperationResult::FailedAfterDelete(error) => {
                // The previous resource no longer exists, so it must not be copied back into this
                // graph. Removing it means it will be created on the next evaluation.
                self.resources.remove(resource_id);

                *failures_count += 1;
                logger::end_action(format!(
                    "Failed after deleting the previous resource: {}",
                    Paint::red(error)
                ));
            }
        }
    }

    /// Copies the previous version of a resource into this graph, or removes the resource if it
    /// did not exist previously. The previous version is also removed if any of its dependencies
    /// are no longer in this graph, since a resource with missing dependencies cannot be evaluated
    /// on the next run.
    fn restore_previous_resource(
        &mut self,
        previous_graph: &ResourceGraph<TResource, TInputs, TOutputs>,
        resource_id: &str,
    ) {
        match previous_graph.resources.get(resource_id) {
            Some(previous_resource)
                if previous_resource
                    .get_dependencies()
                    .iter()
                    .all(|dependency| self.resources.contains_key(dependency)) =>
            {
                self.resources
                    .insert(resource_id.to_owned(), previous_resource.to_owned());
            }
            _ => {
                self.resources.remove(resource_id);
            }
        }
    }

    async fn evaluate_delete<TManager>(
        &self,
        previous_graph: &ResourceGraph<TResource, TInputs, TOutputs>,
        manager: &mut TManager,
        resource_id: &str,
    ) -> OperationResult<TOutputs>
    where
        TManager: ResourceManager<TInputs, TOutputs>,
    {
        let resource = previous_graph.resources.get(resource_id).unwrap();
        let dependency_outputs = previous_graph
            .get_dependency_outputs(resource)
            .expect("Previous graph should be complete.");

        let inputs_hash = resource.get_inputs_hash();
        let dependencies_hash = self.get_dependency_outputs_hash(dependency_outputs.clone());
        logger::start_action(format!(
            "{} Deleting: {}",
            Paint::red("-"),
            resource.get_id()
        ));
        logger::log("Dependencies:");
        logger::log_changeset(get_changeset(&dependencies_hash, &dependencies_hash));
        logger::log("Inputs:");
        logger::log_changeset(get_changeset(&inputs_hash, ""));

        match manager
            .delete(
                resource
                    .get_outputs()
                    .expect("Existing resource should have outputs."),
                dependency_outputs,
            )
            .await
        {
            Ok(()) => OperationResult::SucceededDelete,
            Err(error) => OperationResult::Failed(error),
        }
    }

    async fn evaluate_create_or_update<TManager>(
        &self,
        previous_graph: &ResourceGraph<TResource, TInputs, TOutputs>,
        manager: &mut TManager,
        resource_id: &str,
        allow_purchases: bool,
        skip_resource_prefixes: &[&str],
    ) -> OperationResult<TOutputs>
    where
        TManager: ResourceManager<TInputs, TOutputs>,
    {
        if let Some(prefix) = skip_resource_prefixes
            .iter()
            .find(|prefix| resource_id.starts_with(*prefix))
        {
            logger::start_action(format!(
                "{} Skipping: {}",
                Paint::new("○").dimmed(),
                resource_id
            ));
            return OperationResult::Skipped(format!(
                "Resources with prefix '{}' were skipped by request.",
                prefix
            ));
        }

        let resource = self.resources.get(resource_id).unwrap();
        let inputs_hash = resource.get_inputs_hash();
        let dependency_outputs = self.get_dependency_outputs(resource);

        let previous_resource = previous_graph.resources.get(resource_id);

        if let Some(previous_resource) = previous_resource {
            // Check for changes
            let previous_hash = previous_resource.get_inputs_hash();
            let previous_dependency_outputs = previous_graph
                .get_dependency_outputs(previous_resource)
                .expect("Previous graph should be complete.");
            let previous_dependencies_hash =
                self.get_dependency_outputs_hash(previous_dependency_outputs);

            // TODO: How can we determine between update/noop?
            let dependency_outputs = match dependency_outputs {
                Some(v) => v,
                None => {
                    logger::start_action(format!(
                        "{} Update or Noop: {}",
                        Paint::new("○").dimmed(),
                        resource.get_id(),
                    ));
                    return OperationResult::Skipped(
                        "A dependency failed to produce outputs.".to_owned(),
                    );
                }
            };
            let dependencies_hash = self.get_dependency_outputs_hash(dependency_outputs.clone());

            if previous_hash == inputs_hash && previous_dependencies_hash == dependencies_hash {
                // No changes
                return OperationResult::Noop;
            }

            // This resource has changed
            logger::start_action(format!("{} Updating: {}", Paint::yellow("~"), resource_id));
            logger::log("Dependencies:");
            logger::log_changeset(get_changeset(
                &previous_dependencies_hash,
                &dependencies_hash,
            ));
            logger::log("Inputs:");
            logger::log_changeset(get_changeset(&previous_hash, &inputs_hash));

            let outputs = previous_resource
                .get_outputs()
                .expect("Existing resource should have outputs.");

            let price = match manager
                .get_update_price(
                    resource.get_inputs(),
                    outputs.clone(),
                    dependency_outputs.clone(),
                )
                .await
            {
                Ok(Some(price)) if price > 0 => {
                    if allow_purchases {
                        logger::log("");
                        logger::log(Paint::yellow(format!(
                            "{} Robux will be charged from your account.",
                            price
                        )));
                        Some(price)
                    } else {
                        return OperationResult::Skipped(format!(
                                "Resource would cost {} Robux to create. Give Mantle permission to make purchases with --allow-purchases.",
                                price
                            ));
                    }
                }
                Err(error) => return OperationResult::Failed(error),
                Ok(_) => None,
            };

            if manager.replaces_on_update(&resource.get_inputs()) {
                if let Err(error) = manager.delete(outputs, dependency_outputs.clone()).await {
                    return OperationResult::Failed(error);
                }
                return match manager
                    .create(resource.get_inputs(), dependency_outputs, price)
                    .await
                {
                    Ok(outputs) => OperationResult::SucceededUpdate(outputs),
                    Err(error) => OperationResult::FailedAfterDelete(error),
                };
            }

            match manager
                .update(resource.get_inputs(), outputs, dependency_outputs, price)
                .await
            {
                Ok(outputs) => OperationResult::SucceededUpdate(outputs),
                Err(error) => OperationResult::Failed(error),
            }
        } else {
            // Create
            logger::start_action(format!("{} Creating: {}", Paint::green("+"), resource_id));

            let dependency_outputs = match dependency_outputs {
                Some(v) => v,
                None => {
                    return OperationResult::Skipped(
                        "A dependency failed to produce outputs.".to_owned(),
                    );
                }
            };
            let dependencies_hash = self.get_dependency_outputs_hash(dependency_outputs.clone());

            logger::log("Dependencies:");
            logger::log_changeset(get_changeset(&dependencies_hash, &dependencies_hash));
            logger::log("Inputs:");
            logger::log_changeset(get_changeset("", &inputs_hash));

            let price = match manager
                .get_create_price(resource.get_inputs(), dependency_outputs.clone())
                .await
            {
                Ok(Some(price)) if price > 0 => {
                    if allow_purchases {
                        logger::log("");
                        logger::log(Paint::yellow(format!(
                            "{} Robux will be charged from your account.",
                            price
                        )));
                        Some(price)
                    } else {
                        return OperationResult::Skipped(format!(
                                "Resource would cost {} Robux to create. Give Mantle permission to make purchases with --allow-purchases.",
                                price
                            ));
                    }
                }
                Err(error) => return OperationResult::Failed(error),
                Ok(_) => None,
            };

            match manager
                .create(resource.get_inputs(), dependency_outputs, price)
                .await
            {
                Ok(outputs) => OperationResult::SucceededCreate(outputs),
                Err(error) => OperationResult::Failed(error),
            }
        }
    }

    pub async fn evaluate<TManager>(
        &mut self,
        previous_graph: &ResourceGraph<TResource, TInputs, TOutputs>,
        manager: &mut TManager,
        allow_purchases: bool,
        skip_resource_prefixes: &[&str],
    ) -> Result<EvaluateResults, String>
    where
        TManager: ResourceManager<TInputs, TOutputs>,
    {
        let mut results = EvaluateResults::default();
        let mut failures_count: u32 = 0;

        // Iterate over previous resources in reverse order so that leaf resources are removed first
        let mut previous_resource_order = previous_graph.get_topological_order()?;
        previous_resource_order.reverse();
        for resource_id in previous_resource_order.iter() {
            if self.resources.contains_key(resource_id) {
                continue;
            }

            let operation_result: OperationResult<TOutputs> = self
                .evaluate_delete(previous_graph, manager, resource_id)
                .await;
            self.handle_operation_result(
                &mut results,
                &mut failures_count,
                previous_graph,
                resource_id,
                operation_result,
            );
        }

        let resource_order = self.get_topological_order()?;
        for resource_id in resource_order.iter() {
            let operation_result = self
                .evaluate_create_or_update(
                    previous_graph,
                    manager,
                    resource_id,
                    allow_purchases,
                    skip_resource_prefixes,
                )
                .await;
            self.handle_operation_result(
                &mut results,
                &mut failures_count,
                previous_graph,
                resource_id,
                operation_result,
            );
        }

        if failures_count > 0 {
            Err(format!(
                "Failed {} changes(s) while evaluating the resource graph. See above for more details.",
                failures_count
            ))
        } else {
            Ok(results)
        }
    }

    pub fn diff(
        &mut self,
        previous_graph: &ResourceGraph<TResource, TInputs, TOutputs>,
    ) -> Result<ResourceGraphDiff, String> {
        let mut diff = ResourceGraphDiff {
            removals: BTreeMap::new(),
            additions: BTreeMap::new(),
            changes: BTreeMap::new(),
            dependency_changes: BTreeMap::new(),
        };

        // Iterate over previous resources in reverse order so that leaf resources are removed first
        let mut previous_resource_order = previous_graph.get_topological_order()?;
        previous_resource_order.reverse();

        for resource_id in previous_resource_order.iter() {
            if self.resources.contains_key(resource_id) {
                continue;
            }

            diff.removals.insert(
                resource_id.to_owned(),
                ResourceRemoval {
                    previous_inputs_hash: previous_graph
                        .resources
                        .get(resource_id)
                        .unwrap()
                        .get_inputs_hash(),
                    previous_outputs_hash: previous_graph
                        .resources
                        .get(resource_id)
                        .unwrap()
                        .get_outputs_hash(),
                },
            );
        }

        let resource_order = self.get_topological_order()?;
        for resource_id in resource_order.iter() {
            let resource = self.resources.get(resource_id).unwrap();
            let inputs_hash = resource.get_inputs_hash();

            let previous_resource = previous_graph.resources.get(resource_id);

            if let Some(previous_resource) = previous_resource {
                let previous_hash = previous_resource.get_inputs_hash();
                if previous_hash != inputs_hash {
                    diff.changes.insert(
                        resource_id.to_owned(),
                        ResourceChange {
                            previous_inputs_hash: previous_hash,
                            previous_outputs_hash: previous_resource.get_outputs_hash(),
                            current_inputs_hash: inputs_hash,
                        },
                    );
                } else {
                    let dependencies = resource.get_dependencies();
                    #[allow(clippy::iter_overeager_cloned)]
                    let changed_dependencies: Vec<_> = dependencies
                        .iter()
                        .cloned()
                        .filter(|x| diff.additions.contains_key(x) || diff.changes.contains_key(x))
                        .collect();

                    if !changed_dependencies.is_empty() {
                        diff.dependency_changes.insert(
                            resource_id.to_owned(),
                            ResourceDependencyChange {
                                previous_inputs_hash: previous_hash,
                                previous_outputs_hash: previous_resource.get_outputs_hash(),
                                current_inputs_hash: inputs_hash,
                                changed_dependencies,
                            },
                        );
                    }
                }
            } else {
                diff.additions.insert(
                    resource_id.to_owned(),
                    ResourceAddition {
                        current_inputs_hash: inputs_hash,
                    },
                );
            }
        }

        Ok(diff)
    }
}

#[derive(Serialize)]
pub struct ResourceGraphDiff {
    pub removals: BTreeMap<ResourceId, ResourceRemoval>,
    pub additions: BTreeMap<ResourceId, ResourceAddition>,
    pub changes: BTreeMap<ResourceId, ResourceChange>,
    pub dependency_changes: BTreeMap<ResourceId, ResourceDependencyChange>,
}

#[derive(Serialize)]
pub struct ResourceRemoval {
    pub previous_inputs_hash: String,
    pub previous_outputs_hash: String,
}

#[derive(Serialize)]
pub struct ResourceAddition {
    pub current_inputs_hash: String,
}

#[derive(Serialize)]
pub struct ResourceChange {
    pub previous_inputs_hash: String,
    pub previous_outputs_hash: String,
    pub current_inputs_hash: String,
}

#[derive(Serialize)]
pub struct ResourceDependencyChange {
    pub previous_inputs_hash: String,
    pub previous_outputs_hash: String,
    pub current_inputs_hash: String,
    pub changed_dependencies: Vec<ResourceId>,
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
    struct TestInputs(String);

    #[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
    struct TestOutputs(String);

    #[derive(Clone)]
    struct TestResource {
        id: ResourceId,
        inputs: TestInputs,
        outputs: Option<TestOutputs>,
        dependencies: Vec<ResourceId>,
    }

    impl TestResource {
        fn new(id: &str, inputs: &str, outputs: Option<&str>, dependencies: &[&str]) -> Self {
            Self {
                id: id.to_owned(),
                inputs: TestInputs(inputs.to_owned()),
                outputs: outputs.map(|o| TestOutputs(o.to_owned())),
                dependencies: dependencies.iter().map(|d| (*d).to_owned()).collect(),
            }
        }
    }

    impl Resource<TestInputs, TestOutputs> for TestResource {
        fn get_id(&self) -> ResourceId {
            self.id.clone()
        }
        fn get_inputs_hash(&self) -> String {
            self.inputs.0.clone()
        }
        fn get_outputs_hash(&self) -> String {
            self.outputs
                .as_ref()
                .map(|o| o.0.clone())
                .unwrap_or_default()
        }
        fn get_inputs(&self) -> TestInputs {
            self.inputs.clone()
        }
        fn get_outputs(&self) -> Option<TestOutputs> {
            self.outputs.clone()
        }
        fn get_dependencies(&self) -> Vec<ResourceId> {
            self.dependencies.clone()
        }
        fn set_outputs(&mut self, outputs: TestOutputs) {
            self.outputs = Some(outputs);
        }
    }

    /// Creates produce outputs equal to `out:<inputs>`. Inputs listed in `fail_create` fail.
    /// Resources whose inputs start with `replace:` are replaced on update.
    #[derive(Default)]
    struct TestManager {
        fail_create: Vec<String>,
        calls: Mutex<Vec<String>>,
    }

    impl TestManager {
        fn take_calls(&self) -> Vec<String> {
            std::mem::take(&mut *self.calls.lock().unwrap())
        }
    }

    #[async_trait]
    impl ResourceManager<TestInputs, TestOutputs> for TestManager {
        async fn get_create_price(
            &self,
            _inputs: TestInputs,
            _dependency_outputs: Vec<TestOutputs>,
        ) -> Result<Option<u32>, String> {
            Ok(None)
        }

        async fn create(
            &self,
            inputs: TestInputs,
            _dependency_outputs: Vec<TestOutputs>,
            _price: Option<u32>,
        ) -> Result<TestOutputs, String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("create:{}", inputs.0));
            if self.fail_create.contains(&inputs.0) {
                return Err("429 Too Many Requests".to_owned());
            }
            Ok(TestOutputs(format!("out:{}", inputs.0)))
        }

        async fn get_update_price(
            &self,
            _inputs: TestInputs,
            _outputs: TestOutputs,
            _dependency_outputs: Vec<TestOutputs>,
        ) -> Result<Option<u32>, String> {
            Ok(None)
        }

        async fn update(
            &self,
            inputs: TestInputs,
            _outputs: TestOutputs,
            dependency_outputs: Vec<TestOutputs>,
            price: Option<u32>,
        ) -> Result<TestOutputs, String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("update:{}", inputs.0));
            self.create(inputs, dependency_outputs, price).await
        }

        async fn delete(
            &self,
            outputs: TestOutputs,
            _dependency_outputs: Vec<TestOutputs>,
        ) -> Result<(), String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("delete:{}", outputs.0));
            Ok(())
        }

        fn replaces_on_update(&self, inputs: &TestInputs) -> bool {
            inputs.0.starts_with("replace:")
        }
    }

    type TestGraph = ResourceGraph<TestResource, TestInputs, TestOutputs>;

    #[tokio::test]
    async fn skips_resources_matching_prefix() {
        let previous = TestGraph::new(&[
            TestResource::new("root", "root", Some("out:root"), &[]),
            TestResource::new("placeFile_start", "v1", Some("out:v1"), &["root"]),
        ]);
        let mut next = TestGraph::new(&[
            TestResource::new("root", "root", None, &[]),
            TestResource::new("placeFile_start", "v2", None, &["root"]),
            TestResource::new("placeFile_new", "v1", None, &["root"]),
        ]);
        let mut manager = TestManager::default();

        let results = next
            .evaluate(&previous, &mut manager, false, &["placeFile_"])
            .await
            .unwrap();

        assert_eq!(results.skipped_count, 2);
        assert!(manager.calls.lock().unwrap().is_empty());
        // Existing resource keeps its previous state; new resource is not added
        let resource = next.resources.get("placeFile_start").unwrap();
        assert_eq!(resource.get_inputs(), TestInputs("v1".to_owned()));
        assert_eq!(
            resource.get_outputs(),
            Some(TestOutputs("out:v1".to_owned()))
        );
        assert!(!next.resources.contains_key("placeFile_new"));
    }

    #[tokio::test]
    async fn removes_replaced_resource_and_dependents_when_create_fails_after_delete() {
        let previous = TestGraph::new(&[
            TestResource::new("root", "root", Some("out:root"), &[]),
            TestResource::new("thumb", "replace:a1", Some("out:replace:a1"), &["root"]),
            TestResource::new("order", "order", Some("out:order"), &["root", "thumb"]),
        ]);
        let desired = [
            TestResource::new("root", "root", None, &[]),
            TestResource::new("thumb", "replace:a2", None, &["root"]),
            TestResource::new("order", "order", None, &["root", "thumb"]),
        ];
        let mut next = TestGraph::new(&desired);
        let mut manager = TestManager {
            fail_create: vec!["replace:a2".to_owned()],
            ..Default::default()
        };

        let result = next.evaluate(&previous, &mut manager, false, &[]).await;

        assert!(result.is_err());
        assert_eq!(
            manager.take_calls(),
            vec!["delete:out:replace:a1", "create:replace:a2"]
        );
        // The deleted resource must not be restored, and neither may resources depending on it
        assert!(!next.resources.contains_key("thumb"));
        assert!(!next.resources.contains_key("order"));

        // The saved state can be evaluated again, recreating both resources
        let saved = TestGraph::new(&next.get_resource_list());
        let mut next = TestGraph::new(&desired);
        manager.fail_create.clear();

        let results = next
            .evaluate(&saved, &mut manager, false, &[])
            .await
            .unwrap();

        assert_eq!(results.created_count, 2);
        assert_eq!(
            manager.take_calls(),
            vec!["create:replace:a2", "create:order"]
        );
    }

    #[tokio::test]
    async fn restores_replaced_resource_when_delete_fails() {
        struct FailingDeleteManager(TestManager);

        #[async_trait]
        impl ResourceManager<TestInputs, TestOutputs> for FailingDeleteManager {
            async fn get_create_price(
                &self,
                inputs: TestInputs,
                dependency_outputs: Vec<TestOutputs>,
            ) -> Result<Option<u32>, String> {
                self.0.get_create_price(inputs, dependency_outputs).await
            }
            async fn create(
                &self,
                inputs: TestInputs,
                dependency_outputs: Vec<TestOutputs>,
                price: Option<u32>,
            ) -> Result<TestOutputs, String> {
                self.0.create(inputs, dependency_outputs, price).await
            }
            async fn get_update_price(
                &self,
                inputs: TestInputs,
                outputs: TestOutputs,
                dependency_outputs: Vec<TestOutputs>,
            ) -> Result<Option<u32>, String> {
                self.0
                    .get_update_price(inputs, outputs, dependency_outputs)
                    .await
            }
            async fn update(
                &self,
                inputs: TestInputs,
                outputs: TestOutputs,
                dependency_outputs: Vec<TestOutputs>,
                price: Option<u32>,
            ) -> Result<TestOutputs, String> {
                self.0
                    .update(inputs, outputs, dependency_outputs, price)
                    .await
            }
            async fn delete(
                &self,
                _outputs: TestOutputs,
                _dependency_outputs: Vec<TestOutputs>,
            ) -> Result<(), String> {
                Err("500 Internal Server Error".to_owned())
            }
            fn replaces_on_update(&self, inputs: &TestInputs) -> bool {
                self.0.replaces_on_update(inputs)
            }
        }

        let previous = TestGraph::new(&[
            TestResource::new("root", "root", Some("out:root"), &[]),
            TestResource::new("thumb", "replace:a1", Some("out:replace:a1"), &["root"]),
        ]);
        let mut next = TestGraph::new(&[
            TestResource::new("root", "root", None, &[]),
            TestResource::new("thumb", "replace:a2", None, &["root"]),
        ]);
        let mut manager = FailingDeleteManager(TestManager::default());

        assert!(next
            .evaluate(&previous, &mut manager, false, &[])
            .await
            .is_err());

        // The previous resource still exists, so it is kept as-is
        let resource = next.resources.get("thumb").unwrap();
        assert_eq!(
            resource.get_outputs(),
            Some(TestOutputs("out:replace:a1".to_owned()))
        );
        assert!(manager.0.take_calls().is_empty());
    }
}
