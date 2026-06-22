use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::resource_graph::{Resource, ResourceId};

use super::{
    super::roblox_resource_manager::{FileInputs, RobloxResource},
    v6::ResourceStateV6,
    RobloxInputs,
};

#[derive(Serialize, Deserialize, Clone)]
pub struct ResourceStateV7 {
    pub environments: BTreeMap<String, Vec<RobloxResource>>,
}

impl From<ResourceStateV6> for ResourceStateV7 {
    fn from(state: ResourceStateV6) -> Self {
        let mut environments = BTreeMap::new();

        for (environment_name, resources) in state.environments {
            let mut environment: Vec<RobloxResource> = Vec::new();

            let resource_by_id = &resources
                .iter()
                .map(|resource| (resource.get_id(), resource.to_owned()))
                .collect::<HashMap<_, _>>();

            // The icon is no longer a separate resource; fold each ProductIcon back into the
            // Product it belonged to. A ProductIcon's first (and only) dependency is its parent
            // Product (see the v4 migration), so we key the icons by that product's id.
            let icon_by_product = resources
                .iter()
                .filter_map(|resource| match resource.get_inputs() {
                    RobloxInputs::ProductIcon(file_inputs) => resource
                        .get_dependencies()
                        .first()
                        .map(|product_id| (product_id.clone(), file_inputs)),
                    _ => None,
                })
                .collect::<HashMap<ResourceId, FileInputs>>();

            for resource in resources {
                match resource.get_inputs() {
                    RobloxInputs::Product(mut product_inputs) => {
                        if let Some(icon) = icon_by_product.get(&resource.get_id()) {
                            product_inputs.icon_file_path = Some(icon.file_path.clone());
                            product_inputs.icon_file_hash = Some(icon.file_hash.clone());
                        }

                        let dependencies = resource
                            .get_dependencies()
                            .iter()
                            .map(|id| resource_by_id.get(id).unwrap())
                            .collect::<Vec<_>>();

                        match resource.get_outputs() {
                            Some(outputs) => environment.push(RobloxResource::existing(
                                &resource.get_id(),
                                RobloxInputs::Product(product_inputs),
                                outputs,
                                &dependencies,
                            )),
                            None => environment.push(RobloxResource::new(
                                &resource.get_id(),
                                RobloxInputs::Product(product_inputs),
                                &dependencies,
                            )),
                        }
                    }
                    RobloxInputs::ProductIcon(_) => {}
                    _ => {
                        environment.push(resource.clone());
                    }
                }
            }

            environments.insert(environment_name.to_owned(), environment);
        }

        ResourceStateV7 { environments }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roblox_resource_manager::{
        AssetOutputs, ProductInputs, ProductOutputs, RobloxOutputs,
    };

    fn product(deps: &[&RobloxResource]) -> RobloxResource {
        RobloxResource::existing(
            "product_test",
            RobloxInputs::Product(ProductInputs {
                name: "Test".to_owned(),
                description: "Desc".to_owned(),
                price: 100,
                icon_file_path: None,
                icon_file_hash: None,
            }),
            RobloxOutputs::Product(ProductOutputs {
                asset_id: 1,
                product_id: 1,
            }),
            deps,
        )
    }

    fn state_with(resources: Vec<RobloxResource>) -> ResourceStateV6 {
        let mut environments = BTreeMap::new();
        environments.insert("staging".to_owned(), resources);
        ResourceStateV6 { environments }
    }

    #[test]
    fn folds_product_icon_into_product() {
        let product = product(&[]);
        let product_icon = RobloxResource::existing(
            "productIcon_test",
            RobloxInputs::ProductIcon(FileInputs {
                file_path: "icon.png".to_owned(),
                file_hash: "abc123".to_owned(),
            }),
            RobloxOutputs::ProductIcon(AssetOutputs { asset_id: 2 }),
            &[&product],
        );

        let v7 = ResourceStateV7::from(state_with(vec![product, product_icon]));
        let resources = &v7.environments["staging"];

        // The ProductIcon resource is dropped, leaving only the Product.
        assert_eq!(resources.len(), 1);
        match resources[0].get_inputs() {
            RobloxInputs::Product(inputs) => {
                assert_eq!(inputs.icon_file_path.as_deref(), Some("icon.png"));
                assert_eq!(inputs.icon_file_hash.as_deref(), Some("abc123"));
            }
            _ => panic!("expected a Product resource"),
        }
    }

    #[test]
    fn product_without_icon_is_left_unchanged() {
        let v7 = ResourceStateV7::from(state_with(vec![product(&[])]));
        let resources = &v7.environments["staging"];

        assert_eq!(resources.len(), 1);
        match resources[0].get_inputs() {
            RobloxInputs::Product(inputs) => {
                assert_eq!(inputs.icon_file_path, None);
                assert_eq!(inputs.icon_file_hash, None);
            }
            _ => panic!("expected a Product resource"),
        }
    }
}
