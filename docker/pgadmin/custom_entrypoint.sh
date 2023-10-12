#!/bin/sh

# Call the original entrypoint
/entrypoint.sh "$@"

# Your additional commands to invoke setup.py (example below, adjust accordingly)
python setup.py --load-servers /path/to/your/servers.json

# Keep container running
tail -f /dev/null
