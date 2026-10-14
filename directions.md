There is a venv in venv. uv pip what you need in there. Boto is there already.

Create a cli app in python/ that:

- Accepts a string (like c7g.8xlarge) for an instance type (default to c7g.2xlarge)
- Accepts a region (default to us-east-1)
- Accepts an init file (which does not go in userdata)
