pub mod models;

use std::path::PathBuf;

use reqwest::multipart::Form;

use crate::{
    errors::RobloxApiResult,
    helpers::{get_file_part, handle, handle_as_json},
    models::AssetId,
    RobloxApi,
};

use self::models::{DeveloperProductConfigV2, ListDeveloperProductsResponse};

impl RobloxApi {
    pub async fn create_developer_product(
        &self,
        experience_id: AssetId,
        name: String,
        price: u32,
        description: String,
        icon_file: Option<PathBuf>,
    ) -> RobloxApiResult<DeveloperProductConfigV2> {
        let res = self
            .csrf_token_store
            .send_request(|| async {
                let mut form = Form::new()
                    .text("name", name.clone())
                    .text("description", description.clone())
                    .text("isForSale", true.to_string())
                    .text("price", price.to_string());
                if let Some(icon_file) = &icon_file {
                    form = form.part("imageFile", get_file_part(icon_file).await?);
                }

                Ok(self
                    .client
                    .post(format!(
                        "https://apis.roblox.com/developer-products/v2/universes/{}/developer-products",
                        experience_id
                    ))
                    .multipart(form))
            })
            .await;

        handle_as_json(res).await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_developer_product(
        &self,
        experience_id: AssetId,
        product_id: AssetId,
        name: String,
        price: u32,
        description: String,
        is_for_sale: bool,
        icon_file: Option<PathBuf>,
    ) -> RobloxApiResult<()> {
        let res = self
            .csrf_token_store
            .send_request(|| async {
                let mut form = Form::new()
                    .text("name", name.clone())
                    .text("description", description.clone())
                    .text("isForSale", is_for_sale.to_string())
                    .text("price", price.to_string());
                if let Some(icon_file) = &icon_file {
                    form = form.part("imageFile", get_file_part(icon_file).await?);
                }

                Ok(self
                    .client
                    .patch(format!(
                        "https://apis.roblox.com/developer-products/v2/universes/{}/developer-products/{}",
                        experience_id, product_id
                    ))
                    .multipart(form))
            })
            .await;

        handle(res).await?;

        Ok(())
    }

    pub async fn list_developer_products(
        &self,
        experience_id: AssetId,
        page_token: Option<String>,
    ) -> RobloxApiResult<ListDeveloperProductsResponse> {
        let res = self
            .csrf_token_store
            .send_request(|| async {
                let mut req = self
                    .client
                    .get(format!(
                        "https://apis.roblox.com/developer-products/v2/universes/{}/developer-products/creator",
                        experience_id
                    ))
                    .query(&[("pageSize", 50.to_string())]);
                if let Some(page_token) = &page_token {
                    req = req.query(&[("pageToken", page_token)]);
                }
                Ok(req)
            })
            .await;

        handle_as_json(res).await
    }

    pub async fn get_all_developer_products(
        &self,
        experience_id: AssetId,
    ) -> RobloxApiResult<Vec<DeveloperProductConfigV2>> {
        let mut all_products = Vec::new();

        let mut page_token: Option<String> = None;
        loop {
            let res = self
                .list_developer_products(experience_id, page_token)
                .await?;
            all_products.extend(res.developer_products);

            if res.next_page_token.is_none() {
                break;
            }

            page_token = res.next_page_token;
        }

        Ok(all_products)
    }
}
