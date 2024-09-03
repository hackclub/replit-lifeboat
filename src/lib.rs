pub mod airtable;
pub mod crosisdownload;
pub mod email;
pub mod r2;
pub mod replit_graphql;

pub mod utils {
    use rand::seq::SliceRandom;
    use rand::Rng;

    pub fn random_user_agent() -> String {
        let browsers = vec!["Chrome", "Firefox", "Safari", "Edge", "Opera"];
        let os = vec![
            "Windows NT 10.0",
            "Macintosh; Intel Mac OS X 10_15_7",
            "Linux; Android 10",
            "iPhone; CPU iPhone OS 14_0 like Mac OS X",
        ];
        let engines = vec!["AppleWebKit/537.36", "Gecko/20100101", "KHTML, like Gecko"];

        let browser = browsers.choose(&mut rand::thread_rng()).unwrap();
        let os = os.choose(&mut rand::thread_rng()).unwrap();
        let engine = engines.choose(&mut rand::thread_rng()).unwrap();

        let browser_version = match *browser {
            "Chrome" => format!(
                "{}.0.{}.{}",
                rand::thread_rng().gen_range(70..90),
                rand::thread_rng().gen_range(3000..4000),
                rand::thread_rng().gen_range(100..150)
            ),
            "Firefox" => format!("{}.0", rand::thread_rng().gen_range(70..90)),
            "Safari" => format!(
                "{}.{}.{}",
                rand::thread_rng().gen_range(13..15),
                rand::thread_rng().gen_range(1..5),
                rand::thread_rng().gen_range(1..10)
            ),
            "Edge" => format!(
                "{}.0.{}.{}",
                rand::thread_rng().gen_range(90..100),
                rand::thread_rng().gen_range(800..900),
                rand::thread_rng().gen_range(50..100)
            ),
            "Opera" => format!(
                "{}.0.{}.{}",
                rand::thread_rng().gen_range(60..80),
                rand::thread_rng().gen_range(3000..4000),
                rand::thread_rng().gen_range(100..150)
            ),
            _ => "0.0".to_string(),
        };

        format!(
            "{} ({}; {}) {} Safari/537.36",
            browser, os, engine, browser_version
        )
    }
}
