#!/bin/bash

# Run the instance and capture the output
INSTANCE_OUTPUT=$(aws ec2 run-instances --image-id 'ami-071bc399fae13263b' --instance-type 'c8g.8xlarge' --instance-initiated-shutdown-behavior 'terminate' --key-name 'lk-macbook-pro' --user-data 'CiMgLS0tLS0tLS0tLS0tLS0tLS0tLS0tLS0tLS0tLQojIFN3aXRjaCB0byBlYzItdXNlciBjb250ZXh0CiMgLS0tLS0tLS0tLS0tLS0tLS0tLS0tLS0tLS0tLQpzdSAtIGVjMi11c2VyIC1jICcKc2V0IC1ldXhvIHBpcGVmYWlsCgpIT01FX0RJUj0vaG9tZS9lYzItdXNlcgpXT1JLX0RJUj0iJEhPTUVfRElSL3dvcmsiCgpta2RpciAtcCAiJFdPUktfRElSIgpjZCAiJFdPUktfRElSIgoKSU5TVEFOQ0VfSUQ9JChjdXJsIC1zIGh0dHA6Ly8xNjkuMjU0LjE2OS4yNTQvbGF0ZXN0L21ldGEtZGF0YS9pbnN0YW5jZS1pZCkKCiMhL2Jpbi9iYXNoCnNldCAtZXV4byBwaXBlZmFpbAoKSE9NRV9ESVI9L2hvbWUvZWMyLXVzZXIKQklOX0RJUj0iJEhPTUVfRElSL2JpbiIKV09SS19ESVI9IiRIT01FX0RJUi93b3JrIgoKZG5mIGluc3RhbGwgLXkgdG11eCBhd3NjbGkgY3VybAoKbWtkaXIgLXAgIiRCSU5fRElSIiAiJFdPUktfRElSIgoKIyAtLS0tLS0tLS0tLS0tLS0tLS0tLS0tLS0tLS0tCiMgRmV0Y2ggbGF0ZXN0IGJpbmFyeSAoYWFyY2g2NCkKIyAtLS0tLS0tLS0tLS0tLS0tLS0tLS0tLS0tLS0tCkxBVEVTVD0kKGF3cyBzMyBscyBzMzovL3V0d2lkL2JpbmFyaWVzLyBcCiAgfCBncmVwIGFhcmNoNjQgXAogIHwgc29ydCBcCiAgfCB0YWlsIC1uMSBcCiAgfCBhd2sgIntwcmludCBcJDR9IikKCmF3cyBzMyBjcCAiczM6Ly91dHdpZC9iaW5hcmllcy8kTEFURVNUIiAiJEhPTUVfRElSLyIKdGFyIC14ZiAiJEhPTUVfRElSLyRMQVRFU1QiIC1DICIkSE9NRV9ESVIiCgpjaG1vZCAreCAiJEhPTUVfRElSL3V0d2lkX2F1dG8iIHx8IHRydWUKCg==' --network-interfaces '{"AssociatePublicIpAddress":true,"DeviceIndex":0,"Groups":["sg-056551750d33ebfef"]}' --iam-instance-profile '{"Arn":"arn:aws:iam::183141186035:instance-profile/spotrole1"}' --instance-market-options '{"MarketType":"spot"}' --metadata-options '{"HttpEndpoint":"enabled","HttpPutResponseHopLimit":2,"HttpTokens":"required"}' --private-dns-name-options '{"HostnameType":"ip-name","EnableResourceNameDnsARecord":true,"EnableResourceNameDnsAAAARecord":false}' --count '1')

# Extract instance ID
INSTANCE_ID=$(echo "$INSTANCE_OUTPUT" | jq -r '.Instances[0].InstanceId')
echo "Instance ID: $INSTANCE_ID"

# Wait for instance to be running and get IP address
echo "Waiting for instance to be running..."
aws ec2 wait instance-running --instance-ids "$INSTANCE_ID"

echo "Getting instance IP address..."
INSTANCE_IP=$(aws ec2 describe-instances --instance-ids "$INSTANCE_ID" --query 'Reservations[0].Instances[0].PublicIpAddress' --output text)
echo "Instance IP: $INSTANCE_IP"

# Create stop_instance.sh script
STOP_SCRIPT_NAME="stop_instance.sh"
counter=1
while [ -f "$STOP_SCRIPT_NAME" ]; do
  STOP_SCRIPT_NAME="stop_instance_${counter}.sh"
  counter=$((counter + 1))
done

cat > "$STOP_SCRIPT_NAME" << EOF
#!/bin/bash

# Stop and terminate the instance
echo "Terminating instance: $INSTANCE_ID"
aws ec2 terminate-instances --instance-ids "$INSTANCE_ID"

# Wait for instance to be terminated
echo "Waiting for instance to terminate..."
aws ec2 wait instance-terminated --instance-ids "$INSTANCE_ID"

# Delete this script
echo "Deleting stop script..."
rm -- "\$0"

EOF

chmod +x "$STOP_SCRIPT_NAME"
echo "Created stop script: $STOP_SCRIPT_NAME"
echo "Run ./$STOP_SCRIPT_NAME to terminate the instance" 

