export async function POST(a) {
  // let url = new URL(request.url);
  // let token = url.searchParams.get("token");
  // let email = url.searchParams.get("email");

  console.log(a);

  // Here you can handle the data, e.g., save it, process it, etc.
  // console.log("Token:", token);
  // console.log("Email:", email);

  // Return a JSON response
  return new Response(
    JSON.stringify({
      success: true,
      message: "Form submission received",
      // token,
      // email,
    }),
    {
      status: 200,
      headers: {
        "Content-Type": "application/json",
      },
    },
  );
}
