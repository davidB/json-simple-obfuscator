pub fn main() {}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    #[test]
    fn test_sample() {
        insta::assert_snapshot!(indoc! {r#"
                {
                    "a": "Hello",
                    "id": 123456,
                    "details": {
                        "user": "johnD",
                        "name": "John Doe",
                        "url": "http://example.com/item/123456"
                    }
                }
            "#},);
    }
}
