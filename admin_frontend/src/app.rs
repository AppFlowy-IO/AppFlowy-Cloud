use leptos::{component, view, IntoView};
use leptos_router::{Route, Router, Routes};

#[component]
pub fn App() -> impl IntoView {
    view! {
        <Router>
            <Routes>
                <Route path="/" view=Home/>
                <Route path="/admin" view=Home/>
                <Route path="/admin/settings" view=Settings/>
                <Route path="/admin/users" view=Users/>
            </Routes>
        </Router>
    }
}

#[component]
pub fn Users() -> impl IntoView {
    view! {
        <script
            src="https://unpkg.com/htmx.org@1.9.6"
            integrity="sha384-FhXw7b6AlE/jyjlZH5iHa/tTe9EpJ1Y55RjcgPbjeWMskSxZt1v9qkxLJWNJaGni"
            crossorigin="anonymous"
        ></script>
    }
}

#[component]
pub fn Home() -> impl IntoView {
    view! {
        <script
            src="https://unpkg.com/htmx.org@1.9.6"
            integrity="sha384-FhXw7b6AlE/jyjlZH5iHa/tTe9EpJ1Y55RjcgPbjeWMskSxZt1v9qkxLJWNJaGni"
            crossorigin="anonymous"
        ></script>

        <body>
            <h1>Admin Login</h1>

            <form>
                <table>
                    <tr>
                        <td>
                            <label for="email">Email:</label>
                        </td>
                        <td>
                            <input type="text" id="email" name="email" required/>
                        </td>

                    </tr>
                    <tr>
                        <td>
                            <label for="password">Password:</label>
                        </td>
                        <td>
                            <input type="password" id="password" name="password" required/>
                        </td>

                    </tr>
                </table>
                <button hx-post="/token?grant_type=password"
                        hx-target="#response">
                    Submit
                </button>
            </form>
            <div id="response"></div>
        </body>
    }
}

#[component]
pub fn Settings() -> impl IntoView {
    view! {
        <script
            src="https://unpkg.com/htmx.org@1.9.6"
            integrity="sha384-FhXw7b6AlE/jyjlZH5iHa/tTe9EpJ1Y55RjcgPbjeWMskSxZt1v9qkxLJWNJaGni"
            crossorigin="anonymous"
        ></script>
        <h1>Settings Page</h1>
        <button
            hx-post="https://test.appflowy.cloud/settings"
            hx-trigger="click"
            hx-swap="innerHTML"
            hx-target="#content"
            mustache-template="foo"
        >
            >
                Click Me!
        </button>

        <p id="content">Start</p>
        <template id="foo"></template>

        <nav>
            <h2>"Navigation"</h2>
            <a href="/">"/home"</a>
        </nav>
    }
}
