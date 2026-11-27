package dev.shynewt.shyspace.features.network

import java.net.InetAddress
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class LocalNetworkTest {
    private fun address(literal: String) = InetAddress.getByName(literal)

    @Test
    fun privateAndLinkLocalAddressesAreOnTheLocalNetwork() {
        listOf(
            "192.168.10.175", "10.0.2.2", "172.16.0.1", "169.254.1.1", "100.64.0.1", "100.127.255.254",
            "fd12:3456::1", "fe80::1", "224.0.0.251", "ff02::fb", "255.255.255.255",
        )
            .forEach { assertTrue(it, address(it).isOnLocalNetwork(emptyList())) }
    }

    @Test
    fun theInternetAndThePhoneItselfAreNot() {
        listOf("8.8.8.8", "2606:4700::1111", "127.0.0.1", "::1", "172.32.0.1", "100.128.0.1")
            .forEach { assertFalse(it, address(it).isOnLocalNetwork(emptyList())) }
    }

    @Test
    fun anEndpointIsCheckedByItsHost() {
        assertTrue(endpointIsOnLocalNetwork(onLink = emptyList(), endpoint = "http://192.168.10.175:9000"))
        assertTrue(endpointIsOnLocalNetwork(onLink = emptyList(), endpoint = "http://[fd12:3456::1]:9000/"))
        assertTrue(endpointIsOnLocalNetwork(onLink = emptyList(), endpoint = "https://nas.local"))
        assertFalse(endpointIsOnLocalNetwork(onLink = emptyList(), endpoint = "http://127.0.0.1:9000"))
        assertFalse(endpointIsOnLocalNetwork(onLink = emptyList(), endpoint = "https://203.0.113.7"))
    }

    @Test
    fun anEndpointThatIsntAnAddressIsLeftToTheCoreToRefuse() {
        assertFalse(endpointIsOnLocalNetwork(onLink = emptyList(), endpoint = "not a url"))
        assertFalse(endpointIsOnLocalNetwork(onLink = emptyList(), endpoint = "https://no-such-host.invalid"))
    }

    @Test
    fun aSubnetThePhoneIsDirectlyOnIsLocalWhateverItsRange() {
        val onLink = listOf(Subnet(address("203.0.113.40"), 24), Subnet(address("2001:db8:1::1"), 64))
        assertTrue(address("203.0.113.9").isOnLocalNetwork(onLink))
        assertTrue(address("2001:db8:1::abcd").isOnLocalNetwork(onLink))
        assertFalse(address("203.0.114.9").isOnLocalNetwork(onLink))
        assertFalse(address("2001:db8:2::1").isOnLocalNetwork(onLink))
    }
}
