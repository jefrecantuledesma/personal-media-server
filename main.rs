#[macro_use]
extern crate rocket;
use bcrypt;
use rocket::data::{Limits, ToByteUnit};
use rocket::fs::TempFile;
use rocket::http::{Cookie, CookieJar};
use rocket::launch;
use rocket::response::{Redirect, Responder, Response};
use rocket::Request;
use rocket::{form::Form, get, post, routes};
use rocket::{response, FromForm};
use std::fmt;
use std::fmt::Display;
use std::fs;
use std::io;
use std::io::Cursor;
use std::path::PathBuf;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

enum LoginResult {
    SuccessfulLogin,
    UnsuccessfulLogin,
}

impl Display for LoginResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoginResult::SuccessfulLogin => write!(f, "Login succcessful."),
            LoginResult::UnsuccessfulLogin => write!(f, "Login unsuccessful."),
        }
    }
}

impl<'r> Responder<'r, 'static> for LoginResult {
    fn respond_to(self, _: &'r Request<'_>) -> response::Result<'static> {
        let response_body = self.to_string(); // Utilizes Display impl for conversion to String
        Response::build()
            .sized_body(response_body.len(), Cursor::new(response_body))
            .ok()
    }
}

#[derive(FromForm)]
struct LoginForm {
    username: String,
    password: String,
}

struct LoggedInUser {
    user_id: String,
}

#[post("/login", data = "<login_form>")]
fn login(cookies: &CookieJar<'_>, login_form: Form<LoginForm>) -> LoginResult {
    let login_content = fs::read_to_string("./fake_users.txt")
        .unwrap()
        .trim()
        .to_string();
    let user_pass: Vec<&str> = login_content.split("\n").collect();
    let hashed_pass = user_pass[1].trim();
    let username = user_pass[0].trim();

    // Verify login
    let match_pass = bcrypt::verify(&login_form.password, &hashed_pass).expect("BADBADBAD");
    if login_form.username == username && match_pass == true {
        cookies.add(Cookie::new("jcledesma", "jcledesma"));
        LoginResult::SuccessfulLogin
    } else {
        LoginResult::UnsuccessfulLogin // Adjust as needed
    }
}

#[post("/logout")]
fn logout(cookies: &CookieJar<'_>) -> Redirect {
    cookies.remove(Cookie::named("jcledesma"));
    Redirect::to(uri!(login)) // Redirect to your login route
}

#[rocket::async_trait]
impl<'r> rocket::request::FromRequest<'r> for LoggedInUser {
    type Error = ();

    async fn from_request(
        req: &'r rocket::request::Request<'_>,
    ) -> rocket::request::Outcome<Self, Self::Error> {
        let cookies = req.cookies();
        println!("{:?}", cookies);
        match cookies.get_private("jcledesma") {
            Some(cookie) => rocket::request::Outcome::Success(LoggedInUser {
                user_id: cookie.value().to_string(),
            }),
            None => rocket::request::Outcome::Forward(rocket::http::Status::NotFound),
        }
    }
}

#[get("/music")]
fn list() -> String {
    let contents = fs::read_to_string("./config").expect("Couldn't read.");
    let split_contents: Vec<&str> = contents.split("= ").collect();
    let music_path = String::from(split_contents[1].trim());
    let paths = fs::read_dir(&music_path).expect("Path does not exist.");
    let mut music_list = String::new();
    for path in paths {
        music_list.push_str(&path.unwrap().path().display().to_string());
        music_list.push('\n');
    }
    music_list = music_list.replace(&music_path, "");
    music_list
}

#[post("/upload/<ext>?<album>", data = "<file_form>")]
async fn upload(
    _logged_in_user: LoggedInUser,
    file_form: Form<TempFile<'_>>,
    ext: &str,
    album: Option<String>,
) -> io::Result<String> {
    let temp_file = file_form.into_inner();
    let file_name = temp_file
        .raw_name()
        .expect("no can do")
        .as_str()
        .unwrap()
        .to_string();
    let base_path = PathBuf::new().join("./uploads");

    // If an album name is provided, add it to the path
    let upload_path = if let Some(album_name) = album {
        base_path.join(album_name)
    } else {
        base_path
    };

    let mut path = upload_path.join(file_name);
    path.set_extension(ext);
    println!("{}", path.clone().into_os_string().into_string().unwrap());

    // Ensure the uploads directory exists, including any album subdirectory
    fs::create_dir_all(&upload_path).expect("Could not create upload path.");

    // Open the temporary file for reading
    let mut temp_file = File::open(temp_file.path().ok_or(io::Error::new(
        io::ErrorKind::NotFound,
        "Temporary file not found",
    ))?)
    .await?;

    // Create the destination file
    let mut file = File::create(&path).await?;

    // Stream chunks of data from the temp file to the destination file
    let mut buffer = [0; 16 * 1024]; // 16KB buffer
    loop {
        let bytes_read = temp_file.read(&mut buffer).await?;
        if bytes_read == 0 {
            break; // End of file reached
        }
        file.write_all(&buffer[..bytes_read]).await?;
    }

    Ok(format!(
        "File uploaded successfully to '{}'.",
        path.display()
    ))
}

#[launch]
fn rocket() -> _ {
    rocket::build()
        .mount("/", routes![list, login, logout, upload])
        .configure(rocket::Config {
            limits: Limits::default()
                .limit("file", 2.gigabytes())
                .limit("data-form", 2.gigabytes()),
            ..Default::default()
        })
}
