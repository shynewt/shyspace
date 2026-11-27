package dev.shynewt.shyspace.features.network

import java.net.Inet4Address
import java.net.Inet6Address
import java.net.InetAddress
import java.net.NetworkInterface
import java.net.URI
import java.net.URISyntaxException
import java.net.UnknownHostException

/**
 * Whether reaching [endpoint] goes over the local network, which an app on Android 17 may do only
 * with the local network permission (without it, connections there silently time out). Looks
 * the host up, so it must run off the main thread. An endpoint that isn't a URL, or a host that
 * doesn't resolve, counts as not local: the core then reports the real problem.
 */
fun endpointIsOnLocalNetwork(endpoint: String, onLink: List<Subnet> = connectedSubnets()): Boolean {
    val host = try {
        URI(endpoint).host
    } catch (error: URISyntaxException) {
        null
    } ?: return false
    if (host.endsWith(".local", ignoreCase = true)) return true
    val addresses = try {
        InetAddress.getAllByName(host.removeSurrounding("[", "]"))
    } catch (error: UnknownHostException) {
        return false
    }
    return addresses.any { it.isOnLocalNetwork(onLink) }
}

/** A subnet a network interface is directly on. */
data class Subnet(val address: InetAddress, val prefixLength: Int) {
    fun contains(other: InetAddress): Boolean {
        val mine = address.address
        val theirs = other.address
        if (mine.size != theirs.size) return false
        for (bit in 0 until prefixLength) {
            val mask = 0x80 ushr (bit % 8)
            if (mine[bit / 8].toInt() and mask != theirs[bit / 8].toInt() and mask) return false
        }
        return true
    }
}

/**
 * The subnets this phone is directly on through a broadcast-capable interface (Wi-Fi, Ethernet),
 * which Android counts as local whatever their range. A VPN's point-to-point link isn't one.
 */
fun connectedSubnets(): List<Subnet> = NetworkInterface.getNetworkInterfaces().toList()
    .filter { it.isUp && !it.isLoopback && !it.isPointToPoint }
    .flatMap { it.interfaceAddresses }
    .map { Subnet(it.address, it.networkPrefixLength.toInt()) }

/**
 * Android's local network (developer.android.com/privacy-and-security/local-network-definition):
 * private, link-local, CGNAT, multicast and broadcast addresses, and [onLink] subnets; never this
 * device itself (loopback).
 */
internal fun InetAddress.isOnLocalNetwork(onLink: List<Subnet>): Boolean =
    !isLoopbackAddress && (
        isSiteLocalAddress || isLinkLocalAddress || isMulticastAddress || isUniqueLocal() ||
            isCarrierGradeNat() || isBroadcast() || onLink.any { it.contains(this) }
        )

/** IPv6's private range, fc00::/7, which InetAddress doesn't count as site-local. */
private fun InetAddress.isUniqueLocal() = this is Inet6Address && (address[0].toInt() and 0xfe) == 0xfc

/** 100.64.0.0/10, shared by carriers and by mesh VPNs like Tailscale. */
private fun InetAddress.isCarrierGradeNat() =
    this is Inet4Address && address[0].toInt() and 0xff == 100 && (address[1].toInt() and 0xc0) == 0x40

private fun InetAddress.isBroadcast() = this is Inet4Address && address.all { it == 0xff.toByte() }
