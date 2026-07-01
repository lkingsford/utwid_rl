#!/bin/bash
set -euxo pipefail

# Get IMDSv2 token
TOKEN=$(curl -s -X PUT "http://169.254.169.254/latest/api/token" -H "X-aws-ec2-metadata-token-ttl-seconds: 21600")
# Get instance ID using token
INSTANCE_ID=$(curl -s -H "X-aws-ec2-metadata-token: $TOKEN" http://169.254.169.254/latest/meta-data/instance-id)

if [ -z "$INSTANCE_ID" ]; then
  echo "ERROR: Failed to get instance ID from metadata service" >&2
  exit 1
fi
echo "Instance ID: $INSTANCE_ID"

while true; do
  # Use || true to continue even if aws command fails
  aws s3 cp /home/ec2-user/explore.json \
    "s3://utwid/result/explore-${INSTANCE_ID}.json" || true
  sleep 60
done
