#!/usr/bin/env bash
# billing-alarm.sh — the P0 cost guardrail: a CloudWatch EstimatedCharges
# alarm on an SNS topic that emails you. The topic is the one ROADMAP's
# dark-device alerting will reuse.
#
#   EMAIL=you@example.com ./scripts/billing-alarm.sh          # $25/month line
#   EMAIL=... THRESHOLD_USD=50 ./scripts/billing-alarm.sh
#
# Needs, once, in the console (no API for it): Billing preferences →
# "Receive CloudWatch billing alerts" on; the EstimatedCharges metric only
# exists in us-east-1 and only after that box is ticked. SNS emails a
# confirmation link; the subscription is pending until you click it.
# Idempotent: re-running updates the alarm in place and re-uses the topic.
set -euo pipefail
export AWS_DEFAULT_REGION=us-east-1
EMAIL="${EMAIL:?set EMAIL=<address that gets the alert>}"
THRESHOLD_USD="${THRESHOLD_USD:-25}"
TOPIC_NAME="supervictor-alerts"
ALARM_NAME="supervictor-estimated-charges"

topic="$(aws sns create-topic --name "$TOPIC_NAME" --query TopicArn --output text)"
echo "topic: $topic"
if aws sns list-subscriptions-by-topic --topic-arn "$topic" --query "Subscriptions[?Endpoint=='$EMAIL'].SubscriptionArn" --output text | grep -q .; then
    echo "subscription for $EMAIL exists"
else
    aws sns subscribe --topic-arn "$topic" --protocol email --notification-endpoint "$EMAIL" >/dev/null
    echo "subscribed $EMAIL — confirm the email SNS just sent"
fi

aws cloudwatch put-metric-alarm \
    --alarm-name "$ALARM_NAME" \
    --alarm-description "AWS estimated charges above $THRESHOLD_USD USD this month (supervictor P0 guardrail)" \
    --namespace AWS/Billing --metric-name EstimatedCharges \
    --dimensions Name=Currency,Value=USD \
    --statistic Maximum --period 21600 --evaluation-periods 1 \
    --threshold "$THRESHOLD_USD" --comparison-operator GreaterThanThreshold \
    --treat-missing-data notBreaching \
    --alarm-actions "$topic" --ok-actions "$topic"
aws cloudwatch describe-alarms --alarm-names "$ALARM_NAME" \
    --query 'MetricAlarms[0].[AlarmName,StateValue,Threshold]' --output text
echo "done. If the state stays INSUFFICIENT_DATA for a day, the billing-alerts preference is not on yet."
