use dioxus::prelude::*;

const CARD_CSS: Asset = asset!("/assets/components/card/style.css");

#[component]
pub fn Card(
    #[props(extends=GlobalAttributes)] attributes: Vec<Attribute>,
    children: Element,
) -> Element {
    rsx! {
        document::Link { rel: "stylesheet", href: CARD_CSS }
        div {
            class: "card",
            "data-slot": "card",
            ..attributes,
            {children}
        }
    }
}

#[component]
pub fn CardHeader(
    #[props(extends=GlobalAttributes)] attributes: Vec<Attribute>,
    children: Element,
) -> Element {
    rsx! {
        div {
            class: "card-header",
            "data-slot": "card-header",
            ..attributes,
            {children}
        }
    }
}

#[component]
pub fn CardTitle(
    #[props(extends=GlobalAttributes)] attributes: Vec<Attribute>,
    children: Element,
) -> Element {
    rsx! {
        div {
            class: "card-title",
            "data-slot": "card-title",
            ..attributes,
            {children}
        }
    }
}

#[component]
pub fn CardDescription(
    #[props(extends=GlobalAttributes)] attributes: Vec<Attribute>,
    children: Element,
) -> Element {
    rsx! {
        div {
            class: "card-description",
            "data-slot": "card-description",
            ..attributes,
            {children}
        }
    }
}

#[component]
pub fn CardAction(
    #[props(extends=GlobalAttributes)] attributes: Vec<Attribute>,
    children: Element,
) -> Element {
    rsx! {
        div {
            class: "card-action",
            "data-slot": "card-action",
            ..attributes,
            {children}
        }
    }
}

#[component]
pub fn CardContent(
    #[props(extends=GlobalAttributes)] attributes: Vec<Attribute>,
    children: Element,
) -> Element {
    rsx! {
        div {
            class: "card-content",
            "data-slot": "card-content",
            ..attributes,
            {children}
        }
    }
}

#[component]
pub fn CardFooter(
    #[props(extends=GlobalAttributes)] attributes: Vec<Attribute>,
    children: Element,
) -> Element {
    rsx! {
        div {
            class: "card-footer",
            "data-slot": "card-footer",
            ..attributes,
            {children}
        }
    }
}
