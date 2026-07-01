#!/bin/bash

# Stop and terminate the instance
echo "Terminating instance: "
aws ec2 terminate-instances --instance-ids ""

# Wait for instance to be terminated
echo "Waiting for instance to terminate..."
aws ec2 wait instance-terminated --instance-ids ""

# Delete this script
echo "Deleting stop script..."
rm -- "$0"

