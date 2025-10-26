#!/bin/bash
# setup-network.sh
#
# This script sets up networking for a container by creating a virtual network
# infrastructure that allows the container to communicate with the host and internet.
#
# EXAMPLE:
# When running: sudo ./target/release/cor run /bin/bash
# 1. Container gets isolated network namespace (can't see host network)
# 2. This script creates a virtual "cable" (veth pair) connecting container to host
# 3. Container gets IP 172.18.0.2, can ping host (172.18.0.1) and access internet
# 4. Host acts as a router/gateway for the container
#
# NETWORK TOPOLOGY:
#   Internet
#      |
#   Host (172.18.0.1) ← Bridge (container-br0) ← veth pair → Container (172.18.0.2)
#      |                                                                      |
#   Host's real IP                                                      Container's isolated network

CONTAINER_PID=$1
CONTAINER_IP="172.18.0.2/24"    # IP assigned to the container
BRIDGE_IP="172.18.0.1/24"       # IP of the bridge (acts as gateway)
BRIDGE_NAME="container-br0"     # Name of the virtual bridge

# Create bridge if it doesn't exist
# A bridge is like a virtual switch that connects multiple network interfaces
# like a hub that all containers plug into
if ! ip link show $BRIDGE_NAME &> /dev/null; then
    echo "Creating bridge $BRIDGE_NAME..."
    ip link add name $BRIDGE_NAME type bridge
    ip addr add $BRIDGE_IP dev $BRIDGE_NAME
    ip link set $BRIDGE_NAME up
    echo "Bridge created with IP $BRIDGE_IP"
fi

# Create veth pair (Virtual Ethernet Pair)
# This creates two virtual network interfaces connected by a virtual cable
# Like having two ethernet ports connected by a cable, but virtual
VETH_HOST="veth-${CONTAINER_PID}"        # One end stays on host
VETH_CONTAINER="veth-c-${CONTAINER_PID}" # Other end goes into container

echo "Creating veth pair: $VETH_HOST <-> $VETH_CONTAINER"
ip link add $VETH_HOST type veth peer name $VETH_CONTAINER

# Attach host side to bridge
# This plugs the host end of the virtual cable into our virtual switch
echo "Attaching $VETH_HOST to bridge $BRIDGE_NAME"
ip link set $VETH_HOST master $BRIDGE_NAME
ip link set $VETH_HOST up

# Move container side into container's network namespace
# This moves the container end of the cable into the container's isolated network
# The container can only see this interface, not the host's real network
echo "Moving $VETH_CONTAINER into container namespace (PID $CONTAINER_PID)"
ip link set $VETH_CONTAINER netns $CONTAINER_PID

# Configure container's network (inside its namespace)
# Now we configure the network interface inside the container
echo "Configuring container network interface..."

# Bring up the interface inside the container
nsenter --net=/proc/$CONTAINER_PID/ns/net ip link set $VETH_CONTAINER up

# Assign IP address to the container
nsenter --net=/proc/$CONTAINER_PID/ns/net ip addr add $CONTAINER_IP dev $VETH_CONTAINER

# Set default route through the bridge (172.18.0.1)
# This tells the container: "to reach any IP, send packets to 172.18.0.1"
# The bridge will then route them to the host and eventually to the internet
nsenter --net=/proc/$CONTAINER_PID/ns/net ip route add default via 172.18.0.1

echo "Container network configured:"
echo "  Container IP: $CONTAINER_IP"
echo "  Gateway: 172.18.0.1"
echo "  Interface: $VETH_CONTAINER"

# Enable IP forwarding and NAT on host
# This allows the host to act as a router for the container
echo "Enabling IP forwarding and NAT on host..."

# Enable IP forwarding so host can route packets between networks
sysctl -w net.ipv4.ip_forward=1

# Set up NAT (Network Address Translation) with MASQUERADE
# This allows container traffic to appear as if it's coming from the host
# When container (172.18.0.2) tries to reach google.com:
# 1. Packet goes from 172.18.0.2 → 172.18.0.1 (bridge)
# 2. Host NATs it: changes source IP from 172.18.0.2 to host's real IP
# 3. Packet goes to internet as if it came from the host
# 4. Response comes back to host, gets NAT'd back to 172.18.0.2
iptables -t nat -A POSTROUTING -s 172.18.0.0/24 -j MASQUERADE

echo "Network setup complete!"
echo ""
echo "Container can now:"
echo "  - Ping the host: ping 172.18.0.1"
echo "  - Access internet: ping google.com"
echo "  - Resolve DNS: nslookup google.com"
