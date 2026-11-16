use crate::cli::LogoType;

/// Embedded distribution or OS ASCII art logo with color token metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DistroLogo {
    /// Canonical lowercase logo identifier (e.g. `"arch"`, `"ubuntu"`).
    pub name: &'static str,
    /// Alternative IDs and distro names that resolve to this logo.
    pub aliases: &'static [&'static str],
    /// Standard ASCII art lines using `$1`, `$2`, `$3` color tokens.
    pub lines: &'static [&'static str],
    /// Compact ASCII art lines for `--logo-type small`.
    pub small_lines: &'static [&'static str],
    /// ANSI foreground color codes corresponding to `$1`, `$2`, `$3`.
    pub colors: &'static [&'static str],
    /// Primary accent ANSI color code used for info labels and headers.
    pub accent_color: &'static str,
}

const LINUX_LOGO: DistroLogo = DistroLogo {
    name: "linux",
    aliases: &["tux", "gnu/linux"],
    colors: &["\x1b[1;37m", "\x1b[1;30m", "\x1b[1;33m"],
    accent_color: "\x1b[1;33m",
    lines: &[
        "        $2#####",
        "       $2#######",
        "       $2##$1O$2#$1O$2##",
        "       $2#$3#####$2#",
        "     $2##$1##$3###$1##$2##",
        "    $2#$1##########$2##",
        "   $2#$1############$2##",
        "   $2#$1############$2###",
        "  $3##$2#$1###########$2##$3#",
        "$3######$2#$1#######$2#$3######",
        "$3#######$2#$1#####$2#$3#######",
        "  $3#####$2#######$3#####",
    ],
    small_lines: &[
        "   $2.---.",
        "  $2/ $1o_o $2\\",
        "  $2|:$3/_/$2|",
        " $2//   $1\\ \\",
        "$2(|     $1| )",
        "$3/'\\_   _/`\\",
        "$3\\___)=(___/",
    ],
};

const UBUNTU_LOGO: DistroLogo = DistroLogo {
    name: "ubuntu",
    aliases: &[
        "kubuntu",
        "xubuntu",
        "lubuntu",
        "edubuntu",
        "ubuntu-mate",
        "ubuntu-budgie",
    ],
    colors: &["\x1b[1;31m", "\x1b[1;37m", "\x1b[1;33m"],
    accent_color: "\x1b[1;31m",
    lines: &[
        "                             ....",
        "              $2.',:clooo:  $1.:looooo:.",
        "           $2.;looooooooc  $1.oooooooooo'",
        "        $2.;looooool:,''.  $1:ooooooooooc",
        "       $2;looool;.         $1'oooooooooo,",
        "      $2;clool'             $1.cooooooc.  $2,,",
        "         $2...                $1......  $2.:oo,",
        "  $1.;clol:,.                        $2.loooo'",
        " $1:ooooooooo,                        $2'ooool",
        "$1'ooooooooooo.                        $2loooo.",
        "$1'ooooooooool                         $2coooo.",
        " $1,loooooooc.                        $2.loooo.",
        "   $1.,;;;'.                          $2;ooooc",
        "       $2...                         $2,ooool.",
        "    $2.cooooc.              $1..',,'.  $2.cooo.",
        "      $2;ooooo:.           $1;oooooooc.  $2:l.",
        "       $2.coooooc,..      $1coooooooooo.",
        "         $2.:ooooooolc:. $1.ooooooooooo'",
        "           $2.':loooooo;  $1,oooooooooc",
        "               $2..';::c'  $1.;loooo:'",
    ],
    small_lines: &[
        "         $1_",
        "     $2---($1_$2)",
        " $1_/  $2---  \\",
        "$1(_) $2|   |  |",
        "  $1\\  $2--- _/",
        "     $2---($1_$2)",
    ],
};

const DEBIAN_LOGO: DistroLogo = DistroLogo {
    name: "debian",
    aliases: &["raspbian"],
    colors: &["\x1b[1;31m", "\x1b[31m", "\x1b[1;37m"],
    accent_color: "\x1b[1;31m",
    lines: &[
        "       $2_,met$$$$$gg.",
        "    $2,g$$$$$$$$$$$$$$$P.",
        "  $2,g$$P\"     \"\"\"Y$$.\".",
        " $2,$$P'              `$$$.",
        "$2',$$P       ,ggs.     `$$b:",
        "$2`d$$'     ,$P\"'   $1.    $2$$$",
        " $2$$P      d$'     $1,    $2$$P",
        " $2$$:      $$.   $1-    $2,d$$'",
        " $2$$;      Y$b._   _,d$P'",
        " $2Y$$.    $1`.$2`\"Y$$$$P\"'",
        "  $2`$$b      $1\"-.__",
        "   $2`Y$$",
        "    $2`Y$$.",
        "      $2`$$b.",
        "        $2`Y$$b.",
        "           $2`\"Y$b._",
        "               `\"\"\"",
    ],
    small_lines: &[
        "  $1_____",
        " $1/  __ \\",
        "$1|  /    |",
        "$1|  \\___-",
        " $1-_",
        "   $1--_",
    ],
};

const ARCH_LOGO: DistroLogo = DistroLogo {
    name: "arch",
    aliases: &["archlinux", "arch-linux", "arco", "arcolinux"],
    colors: &["\x1b[1;36m", "\x1b[36m", "\x1b[1;37m"],
    accent_color: "\x1b[1;36m",
    lines: &[
        "                  $1-`",
        "                 $1.o+`",
        "                $1`ooo/",
        "               $1`+oooo:",
        "              $1`+oooooo:",
        "              $1-+oooooo+:",
        "            $1`/:-:++oooo+:",
        "           $1`/++++/+++++++:",
        "          $1`/++++++++++++++:",
        "         $1`/+++o$2oooooooo$1oooo/`",
        "        $1./$2ooosssso++osssssso$1+`",
        "       $2.oossssso-````/ossssss+`",
        "      $2-osssssso.      :ssssssso.",
        "     $2:osssssss/        osssso+++.",
        "    $2/ossssssss/        +ssssooo/-",
        "  $2`/ossssso+/:-        -:/+osssso+-",
        " $2`+sso+:-`                 `.-/+oso:",
        "$2`++:.                           `-/+/",
        "$2.`                                 `/",
    ],
    small_lines: &[
        "      $1/\\",
        "     $1/  \\",
        "    $1/\\   \\",
        "   $2/      \\",
        "  $2/   ,,   \\",
        " $2/   |  |  -\\",
        "$2/_-''    ''-_\\",
    ],
};

const FEDORA_LOGO: DistroLogo = DistroLogo {
    name: "fedora",
    aliases: &["fedora-linux"],
    colors: &["\x1b[1;34m", "\x1b[1;37m", "\x1b[34m"],
    accent_color: "\x1b[1;34m",
    lines: &[
        "             $1.',;::::;,'",
        "         $1.';:cccccccccccc:;.'",
        "      $1.;cccccccccccccccccccccc;.",
        "    $1.:cccccccccccccccccccccccccc:.",
        "  $1.;ccccccccccccc;$2.:dddl:.$1;ccccccc;.",
        " $1.:ccccccccccccc;$2OWMKOOXMWd$1;ccccccc:.",
        "$1.:ccccccccccccc;$2KMMc$1;cc;$2xMMc$1;ccccccc:.",
        "$1,cccccccccccccc;$2MMM.$1;cc;$2;WW:$1;cccccccc,",
        "$1:cccccccccccccc;$2MMM.$1;cccccccccccccccc:",
        "$1:ccccccc;$2oxOOOo$1;$2MMM0OOk.$1;cccccccccccc:",
        "$1cccccc;$20MMKxdd:$1;$2MMMkddc.$1;cccccccccccc;",
        "$1ccccc;$2XM0'$1;cccc;$2MMM.$1;cccccccccccccccc'",
        "$1ccccc;$2MMo$1;ccccc;$2MMW.$1;ccccccccccccccc;",
        "$1ccccc;$20MNc.$1ccc$2.xMMd$1;ccccccccccccccc;",
        "$1cccccc;$2dNMWXXXWM0:$1;cccccccccccccc:,",
        "$1cccccccc;$2.:odl:.$1;cccccccccccccc:,.",
        "$1:cccccccccccccccccccccccccccc:'.",
        "$1.:cccccccccccccccccccccc:;,..",
        "  $1'::cccccccccccccc::;,.''",
    ],
    small_lines: &[
        "      $1_____",
        "     $1/   __)$2\\",
        "     $1|  /  $2\\ \\",
        "  $2__$1_|  |_$2_/ /",
        " $2/ $1(_    _$2)_/",
        "$2/ /  $1|  |",
        "$2\\ \\__$1|  |",
        " $2\\___$1(  |",
    ],
};

const NIXOS_LOGO: DistroLogo = DistroLogo {
    name: "nixos",
    aliases: &["nix"],
    colors: &["\x1b[1;34m", "\x1b[1;36m", "\x1b[1;37m"],
    accent_color: "\x1b[1;36m",
    lines: &[
        "          $1::::.    $2':::::     ::::'",
        "          $1':::::    $2':::::.  ::::'",
        "            $1:::::     $2':::::::::'",
        "      $2......::::::     $1'::::::'",
        "    $2.::::::::::::::::   $1::::::.",
        "   $2:::::::::::::::::'    $1':::::.",
        "        $1.:::::'           $2':::::.",
        "      $1.:::::'      $2......   ':::::.",
        "    $1.:::::'       $2::::::::   ':::::.",
        "   $1::::::         $2'::::::'     ::::::",
        "    $1':::::.   $2......           .::::'",
        "      $1':::::. $2:::::::        .:::::'",
        "        $1':::::.':::::      .:::::'",
        "    $2.:::::::::::::::::    $1::::::",
        "   $2'::::::::::::::::'      $1':::::.",
        "            $1.:::::     $2.:::::::::.",
        "          $1.:::::'    $2.:::::' ':::::.",
        "          $1::::'     $2.:::::'   ':::::",
    ],
    small_lines: &[
        "  $1\\\\  $2\\\\ //",
        " $1==\\\\__$2\\/ //",
        "   $1//   $2\\//",
        "$1==//     $2//==",
        " $2//\\___$1//",
        "$2// /\\\\  \\\\",
    ],
};

const ALPINE_LOGO: DistroLogo = DistroLogo {
    name: "alpine",
    aliases: &["alpinelinux"],
    colors: &["\x1b[1;34m", "\x1b[1;37m", "\x1b[34m"],
    accent_color: "\x1b[1;34m",
    lines: &[
        "       $1.hddddddddddddddddddddddh.",
        "      $1:dddddddddddddddddddddddddd:",
        "     $1/dddddddddddddddddddddddddddd/",
        "    $1+dddddddddddddddddddddddddddddd+",
        "  $1`sdddddddddddddddddddddddddddddddds`",
        " $1`ydddddddddddd$2++hdddddddddddddddddddy`",
        "$1.hddddddddddd$2+`  `+ddddh:-sdddddddddddh.",
        "$1hdddddddddd$2+`      `+y:    .sddddddddddh",
        "$1ddddddddh$2+`   `//`   `.`     -sdddddddddd",
        "$1ddddddh$2+`   `/hddh/`   `:s-    -sdddddddd",
        "$1ddddh$2+`   `/+/dddddh/`   `+s-    -sdddddd",
        "$1ddd$2+`   `/o` :dddddddh/`   `oy-    .yddd",
        "$1hdddyo$2+`     `dddddddddh/`   `oy-  .ydddh",
        "$1.hdddddddddddddddddddddddh/`   `+ydddddh.",
        " $1`ydddddddddddddddddddddddddh/` `ydddddy`",
        "  $1`sdddddddddddddddddddddddddddddddddds`",
        "    $1+dddddddddddddddddddddddddddddd+",
        "     $1/dddddddddddddddddddddddddddd/",
        "      $1:dddddddddddddddddddddddddd:",
        "       $1.hddddddddddddddddddddddh.",
    ],
    small_lines: &[
        "   $1/\\ /\\",
        "  $1/  \\  \\",
        " $1/ /\\ \\  \\",
        "$1/ /  \\ \\  \\",
        "$1\\ \\   \\ \\ /",
        " $1\\/    \\/",
    ],
};

const MANJARO_LOGO: DistroLogo = DistroLogo {
    name: "manjaro",
    aliases: &["manjaro-arm"],
    colors: &["\x1b[1;32m", "\x1b[32m", "\x1b[1;37m"],
    accent_color: "\x1b[1;32m",
    lines: &[
        "$1||||||||||||||  |||||||",
        "$1||||||||||||||  |||||||",
        "$1||||||||||||||  |||||||",
        "$1|||||||         |||||||",
        "$1|||||||  |||||| |||||||",
        "$1|||||||  |||||| |||||||",
        "$1|||||||  |||||| |||||||",
        "$1|||||||  |||||| |||||||",
        "$1|||||||  |||||| |||||||",
        "$1|||||||  |||||| |||||||",
        "$1|||||||  |||||| |||||||",
        "$1|||||||  |||||| |||||||",
        "$1|||||||  |||||| |||||||",
        "$1|||||||  |||||| |||||||",
    ],
    small_lines: &[
        "$1|||||  |||",
        "$1||     |||",
        "$1|| ||| |||",
        "$1|| ||| |||",
        "$1|| ||| |||",
    ],
};

const ENDEAVOUROS_LOGO: DistroLogo = DistroLogo {
    name: "endeavouros",
    aliases: &["endeavour"],
    colors: &["\x1b[1;35m", "\x1b[1;31m", "\x1b[1;34m"],
    accent_color: "\x1b[1;35m",
    lines: &[
        "                     $2./o.",
        "                   $2./sssso-",
        "                 $2`:osssssss+-",
        "               $2`:+sssssssssso/.",
        "             $2`-/ossssssssssssso/.",
        "           $2`-/+sssssssssssssssso+:`",
        "         $2`-:/+sssssssssssssssssso+/.",
        "       $2`.://osssssssssssssssssssso++-",
        "      $2.://+ssssssssssssssssssssssso++:",
        "    $2.:///ossssssssssssssssssssssssso++:",
        "  $2`:////ssssssssssssssssssssssssssso+++.",
        "$2`-////+ssssssssssssssssssssssssssso++++-",
        " $2`..-+oosssssssssssssssssssssssso+++++/`",
        "   $3./++++++++++++++++++++++++++++++/:.",
        "  $3`:::::::::::::::::::::::::::::::-``",
    ],
    small_lines: &[
        "      $2/\\",
        "    $2//  \\\\",
        "   $2//    \\\\",
        " $2//      \\\\",
        "$2//________\\\\",
        "$3`----------`",
    ],
};

const VOID_LOGO: DistroLogo = DistroLogo {
    name: "void",
    aliases: &["voidlinux"],
    colors: &["\x1b[1;32m", "\x1b[32m", "\x1b[1;37m"],
    accent_color: "\x1b[1;32m",
    lines: &[
        "                $1__.;=====;.__",
        "            $1_.=+==++=++=+=+===;.",
        "             $1-=+++=+===+=+=+++++=.",
        "        $1.     $2-=:``     `--==+=++==.",
        "       $1_vi,    $2`            --+=++++:",
        "      $1.uvnvi.       $1_._       $2-==+==+.",
        "     $1.vvnvnI`    $1.;==|==;.     $2:|=||=|.",
        "$2+QmQQm$1pvvnv; $2_yYsyQQWUUQQQm #QmQ#$1:`QQQW$",
        " $2QQQQW$1unvnv;$2dQ#   $1.vvnvn;  $2QQQQQ$1;QQQQ#",
        "  $2\"\"\"\"$1uvnvn; $2QQQ  $1vvnvnv;  $2QQQQQ$1;QQQQ\"",
        "      $1vvnvnv; $2\"W  $1vvnvnv'  $2`\"\"\"\"`",
        "      $1`vvnvnv.    $2-==+==-.   $1_vn;",
        "       $1`vvnvnv.            $1.uvnv'",
        "        $1`vvnvnv=-..____..-==vnv'",
        "          $1`vvnvnvnvnvnvnvnvnv'",
        "             $1`\"==++++++++==\"`",
    ],
    small_lines: &[
        "   $1_______",
        " $1/  ____ \\",
        "$1|  / __ \\ |",
        "$1| | (__) ||",
        " $1\\ \\____//",
        "  $1`-----'",
    ],
};

const GENTOO_LOGO: DistroLogo = DistroLogo {
    name: "gentoo",
    aliases: &["funtoo"],
    colors: &["\x1b[1;35m", "\x1b[1;37m", "\x1b[35m"],
    accent_color: "\x1b[1;35m",
    lines: &[
        "         $1-/oyddmdhs+:.",
        "     $1-odNMMMMMMMMNNmhy+-`",
        "   $1-yNMMMMMMMMMMMNNNmmdhy+-",
        " $1`omMMMMMMMMMMMMNmdmmmmddhhy/`",
        " $1omMMMMMMMMMMMNhhyyyohmdddhhhdo`",
        "$1.ydMMMMMMMMMMdhs++so/smdddhhhhdm+`",
        " $1oyhdmNMMMMMMMNdyooydmddddhhhhyhNd.",
        "  $1:oyhhdNNMMMMMMMNNNmmdddhhhhhyymMh",
        "    $1.:+sydNMMMMMNNNmmmdddhhhhhhmMmy",
        "       $1/mMMMMMMNNNmmmdddhhhhhmMNhs:",
        "    $1`oNMMMMMMMNNNmmmddddhhdmMNhs+`",
        "  $1`sNMMMMMMMMNNNmmmdddddmNMmhs/.",
        " $1/NMMMMMMMMNNNNmmmdddmNMNdso:`",
        "$1+MMMMMMMNNNNNmmmmdmNMNdso/-",
        "$1yMMNNNNNNNmmmmmNNMmhs+/-`",
        "$1/hMMNNNNNNNNMNdhs++/-`",
        "$1`/ohdmmddhys+++/:.`",
        "  $1`-//////:--.",
    ],
    small_lines: &[
        "  $1.-----.",
        " $1/  ___  \\",
        "$1|  /   \\  |",
        "$1|  \\___/  |",
        " $1\\       /",
        "  $1`-----'",
    ],
};

const OPENSUSE_LOGO: DistroLogo = DistroLogo {
    name: "opensuse",
    aliases: &[
        "opensuse-tumbleweed",
        "opensuse-leap",
        "opensuse-slowroll",
        "suse",
        "sles",
    ],
    colors: &["\x1b[1;32m", "\x1b[1;37m", "\x1b[32m"],
    accent_color: "\x1b[1;32m",
    lines: &[
        "           $1.;ldkO0000Okdl;.",
        "       $1.;d00xl:^''''''^:ok00d;.",
        "     $1.d00l'                'o00d.",
        "   $1.d0Kd'  $2Okxol:;,.          $1:O0d.",
        "  $1.OK$2KKK0kOKKKKKKKKKKOxo:,      $1lKO.",
        " $1,0K$2KKKKKKKKKKKKKKK0P^,,,^0K:$1    ;K0,",
        " $1dK$2KKKKKKKKKKKKKKKk$1. $2o0$1K$20:$1     'Kd",
        "$1;K$2KKKKKKKKKKKKKKKKK$1. $2o0K0:$1      ;K;",
        "$1:K$2KKKKKKKKKKKKKKKKKK$1.$2:dOKo$1      :K:",
        "$1:K$2KKKKKKKKKKKKKKKKKKKx$1'         :K:",
        "$1;K$2KKKKKKKKKKKKKKK0d,$1            ;K;",
        " $1dK$2KKKKKKKKKKKKKK0:$1            .Kd",
        " $1,0K$2KKKKKKKKKKKKKKK0xoc::;;;;::o0,",
        "  $1.OK$2KKKKKKKKKKKKKKKKKKKKKKKKKKO.",
        "   $1.d0K$2KKKKKKKKKKKKKKKKKKKKKK0d.",
        "     $1.d00xl:^''''''''''^:ok00d.",
        "       $1.;d00kdl;......;ldk00d;.",
        "           $1.;ldkO0000Okdl;.",
    ],
    small_lines: &[
        "  $1.----.",
        " $1/  __  \\",
        "$1|  |  o |",
        "$1|   \\___/",
        " $1\\____/",
    ],
};

const POP_LOGO: DistroLogo = DistroLogo {
    name: "pop",
    aliases: &["pop_os", "pop-os", "popos"],
    colors: &["\x1b[1;36m", "\x1b[1;37m", "\x1b[36m"],
    accent_color: "\x1b[1;36m",
    lines: &[
        "             $1/////////////",
        "         $1/////////////////////",
        "      $1///////$2*767$1////////////////",
        "    $1//////$27676767676*$1//////////////",
        "   $1/////$276767$1//$27676767$1//////////////",
        "  $1/////$2767676$1///$2*76767$1///////////////",
        " $1///////$2767676$1///$276767$1.///$27676*$1///////",
        "$1/////////$2767676$1//$276767$1///$2767676$1////////",
        "$1//////////$276767676767$1////$276767$1/////////",
        "$1///////////$276767676$1//////$27676$1//////////",
        "$1////////////,$27676$1,///////$2767$1///////////",
        "$1/////////////*$27676$1///////$276$1////////////",
        "$1///////////////$27676$1//////////////////",
        " $1///////////////$2767$1///////////////////",
        "  $1//////////////$27676$1/////////////////",
        "   $1////////////$276767$1///////////////",
        "    $1///////////$276767$1/////////////",
        "      $1/////////////////////////",
        "         $1///////////////////",
    ],
    small_lines: &[
        "  $1_____",
        " $1/  _  \\",
        "$1|  |_|  |",
        "$1|   ___/",
        "$1|  |  $2.",
        " $1\\_|  $2*",
    ],
};

const MINT_LOGO: DistroLogo = DistroLogo {
    name: "mint",
    aliases: &["linuxmint", "linux-mint"],
    colors: &["\x1b[1;32m", "\x1b[1;37m", "\x1b[32m"],
    accent_color: "\x1b[1;32m",
    lines: &[
        "$1             ...-:::::-...",
        "$1          .-MMMMMMMMMMMMMMM-.",
        "$1      .-MMMM`..-:::::::-..`MMMM-.",
        "$1    .:MMMM.:MMMMMMMMMMMMMMM:.MMMM:.",
        "$1   -MMM-M---MMMMMMMMMMMMMMMMMMM.MMM-",
        "$1 `:MMM:MM`  :MMMM:....::-...-MMMM:MMM:`",
        "$1 :MMM:MMM`  :MM:`  ``    ``  `:MMM:MMM:",
        "$1.MMM.MMMM`  :MM.  -MM.  .MM-  `MMMM.MMM.",
        "$1:MMM:MMMM`  :MM.  -MM-  .MM:  `MMMM-MMM:",
        "$1:MMM:MMMM`  :MM.  -MM-  .MM:  `MMMM:MMM:",
        "$1:MMM:MMMM`  :MM.  -MM-  .MM:  `MMMM-MMM:",
        "$1.MMM.MMMM`  :MM:--:MM:--:MM:  `MMMM.MMM.",
        "$1 :MMM:MMM-  `-MMMMMMMMMMMM-`  -MMM-MMM:",
        "$1  :MMM:MMM:`                `:MMM:MMM:",
        "$1   .MMM.MMMM:--------------:MMMM.MMM.",
        "$1     '-MMMM.-MMMMMMMMMMMMMMM-.MMMM-'",
        "$1       '.-MMMM``--:::::--``MMMM-.'",
        "$1            '-MMMMMMMMMMMMM-'",
        "$1               ``-:::::-``",
    ],
    small_lines: &[
        "  $1_______",
        " $1|  _ _  |",
        " $1| | | | |",
        " $1| | | | |",
        " $1|  ---  |",
        "  $1`-----'",
    ],
};

const KALI_LOGO: DistroLogo = DistroLogo {
    name: "kali",
    aliases: &["kalilinux", "kali-linux"],
    colors: &["\x1b[1;34m", "\x1b[1;37m", "\x1b[34m"],
    accent_color: "\x1b[1;34m",
    lines: &[
        "..............",
        "            ..,;:ccc,.",
        "          ......''';lxO.",
        ".....''''..........,:ld;",
        "           .';;;:::;,,.x,",
        "      ..'''.            0Xxoc:,.  ...",
        "  ....                ,ONkc;,;cokOdc',.",
        " .                   OMo           ':$2dd$1o.",
        "                    dMc               :OO;",
        "                    0M.                 .:o.",
        "                    ;Wd",
        "                     ;XO,",
        "                       ,d0Odlc;,..",
        "                           ..',;:cdOOd::,.",
        "                                    .:d;.':;.",
        "                                       'd,  .'",
        "                                         ;l   ..",
        "                                          .o",
        "                                            c",
        "                                            .'",
    ],
    small_lines: &[
        "   $1____",
        "  $1/  __\\__",
        " $1/  /     \\",
        "$1|  |  \\___/",
        " $1\\  \\_",
        "  $1`---'",
    ],
};

const ROCKY_LOGO: DistroLogo = DistroLogo {
    name: "rocky",
    aliases: &["rockylinux", "rocky-linux"],
    colors: &["\x1b[1;32m", "\x1b[32m", "\x1b[1;37m"],
    accent_color: "\x1b[1;32m",
    lines: &[
        "          $1__wgl@w.",
        "       $1_g@P\"   \"Y@g_",
        "     $1g@P           Y@g",
        "    $1@@'             '@@",
        "   $1@@       _g@g_     @@",
        "  $1@@'     _@@P\"Y@@_   '@@",
        "  $1@@     g@P     Y@g   @@",
        "  $1@@.   @@'       '@@ .@@",
        "   $1@@. g@P         Y@g@@",
        "    $1Y@@@P           Y@@P",
        "     $1\"Y@@_         _@@P\"",
        "       $1\"Y@@g_____g@@P\"",
        "          $1\"\"YYYYY\"\"",
    ],
    small_lines: &[
        "  $1.----.",
        " $1/  /\\  \\",
        "$1|  /  \\  |",
        "$1| /    \\ |",
        " $1\\______/",
    ],
};

const ALMA_LOGO: DistroLogo = DistroLogo {
    name: "alma",
    aliases: &["almalinux", "alma-linux"],
    colors: &["\x1b[1;31m", "\x1b[1;33m", "\x1b[1;34m"],
    accent_color: "\x1b[1;34m",
    lines: &[
        "         $1'c:.",
        "        $1lkkkx, ..       $2..   ,cc,",
        "        $1okkkk:ckkx'  $2.lxkkx.okkkkd",
        "        $1.:llcokkx'  $2:kkkxkko:xkkd,",
        "      $1.xkkkkdood:  $2;kx,  .lkxlll;",
        "       $1xkkx.       $2xk'     xkkkkk:",
        "       $1'xkx.       $2xk,     xkkkkk.",
        "  $3.ok: $1.xkx.       $2:kx.  .'xkkx.",
        " $3okkkkc$1;dd.         $2:xkkkkkko.",
        "$3.kkkkkkx.           $2.:llll:.",
        "$3.xkkkkko",
        "  $3'okkkx.",
        "    $3':;'",
    ],
    small_lines: &[
        "  $1*  $2*",
        " $1/ \\/ \\",
        "$3*  $1\\/$2  *",
        " $3\\    /",
        "  $3`--'",
    ],
};

const CACHYOS_LOGO: DistroLogo = DistroLogo {
    name: "cachyos",
    aliases: &["cachy"],
    colors: &["\x1b[1;36m", "\x1b[36m", "\x1b[1;32m"],
    accent_color: "\x1b[1;36m",
    lines: &[
        "           $1.-------------------------:",
        "          $1.+========================.",
        "         $1:++===$2+++++++++++++++$1======-",
        "        $1:++==$2++++++++++++++++++$1====.",
        "       $1:++==$2++++++++++$1=====-",
        "      $1:++==$2++++++++$1===-",
        "     $1:++==$2++++++$1===-       $3.::.",
        "    $1:++==$2+++++$1===-      $3.:++++:.",
        "   $1:++==$2+++++$1===-     $3.:++++++++.",
        "    $1:++==$2+++++$1===-     $3`:++++:`",
        "     $1:++==$2++++++$1===-",
        "      $1:++==$2++++++++$1===-",
        "       $1:++==$2++++++++++$1=====-",
        "        $1:++==$2++++++++++++++++++$1====.",
        "         $1:++===$2+++++++++++++++$1======-",
        "          $1.+========================.",
        "           $1`-------------------------:",
    ],
    small_lines: &[
        "   $1.----.",
        "  $1/  $2++  $1\\",
        " $1|  $2++ $3o  $1|",
        "  $1\\  $2++  $1/",
        "   $1`----'",
    ],
};

const MACOS_LOGO: DistroLogo = DistroLogo {
    name: "macos",
    aliases: &["darwin", "osx", "mac"],
    colors: &["\x1b[1;32m", "\x1b[1;33m", "\x1b[1;31m"],
    accent_color: "\x1b[1;32m",
    lines: &[
        "                    $1c.'",
        "                 $1,xNMM.",
        "               $1.OMMMMo",
        "               $1lMM\"",
        "     $1.;loddo:.  .olloddol;.",
        "   $1cKMMMMMMMMMMNWMMMMMMMMMM0:",
        " $2.KMMMMMMMMMMMMMMMMMMMMMMMWd.",
        " $2XMMMMMMMMMMMMMMMMMMMMMMMX.",
        "$3;MMMMMMMMMMMMMMMMMMMMMMMM:",
        "$3:MMMMMMMMMMMMMMMMMMMMMMMM:",
        "$3.MMMMMMMMMMMMMMMMMMMMMMMMX.",
        " $3kMMMMMMMMMMMMMMMMMMMMMMMMWd.",
        "  $1'XMMMMMMMMMMMMMMMMMMMMMMMMk",
        "   $1'XMMMMMMMMMMMMMMMMMMMMK.",
        "     $1kMMMMMMMMMMMMMMMMMMd",
        "      $1;KMMMMMMMWXXWMMMMMk.",
        "        $1\"cooc*\"    \"*coo'\"",
    ],
    small_lines: &[
        "     $1.:'",
        "  $1__ :'__",
        "$2.'`__`-'__``.",
        "$3:__________.-'",
        "$3:_________:",
        " $1`--.__.--'",
    ],
};

const WINDOWS_LOGO: DistroLogo = DistroLogo {
    name: "windows",
    aliases: &["win", "msys2", "cygwin"],
    colors: &["\x1b[1;36m", "\x1b[1;34m", "\x1b[1;37m"],
    accent_color: "\x1b[1;36m",
    lines: &[
        "        $1,.=:!!t3Z3z.,",
        "       $1:tt:::tt333EE3",
        "       $1Et:::ztt33EEEL $2@Ee.,      ..,",
        "      $1;tt:::tt333EE7 $2;EEEEEEttttt33#",
        "     $1:Et:::zt333EEQ. $2$EEEEEttttt33QL",
        "     $1it::::tt333EEF $2@EEEEEEttttt33F",
        "    $1;3=*^```\"*4EEV $2:EEEEEEttttt33@.",
        "    $2,.=::::!t=., $1` $2@EEEEEEttttt33QF",
        "   $2;::::::::zt33)   $1\"4EEEtttji3P*",
        "  $2:t::::::::tt33.$1:Z3z..  `` ,..g.",
        "  $2i::::::::zt33F $1AEEEtttt::::ztF",
        " $2;:::::::::t33V $1;EEEttttt::::t3",
        " $2E::::::::zt33L $1@EEEtttt::::z3F",
        "$2{3=*^```\"*4E3) $1;EEEtttt:::::tZ`",
        "             $1` $1:EEEEtttt::::z7",
        "                 $1\"VEzjt:;;z>*`",
    ],
    small_lines: &[
        "$1,---, ,---,",
        "$1|   | |   |",
        "$1'---' '---'",
        "$2,---, ,---,",
        "$2|   | |   |",
        "$2'---' '---'",
    ],
};

const UNKNOWN_LOGO: DistroLogo = DistroLogo {
    name: "unknown",
    aliases: &["other"],
    colors: &["\x1b[1;37m", "\x1b[37m", "\x1b[1;30m"],
    accent_color: "\x1b[1;37m",
    lines: &[
        "     $1.---.",
        "    $1/     \\",
        "    $1\\.@-@./",
        "    $1/`\\_/`\\",
        "   $1//  _  \\\\",
        "  $1| \\     )|_",
        " $1/`\\_`>  <_/ \\",
        " $1\\__/'---'\\__/",
    ],
    small_lines: &["  $1.---.", " $1/  ?  \\", " $1\\     /", "  $1`---'"],
};

/// Registry of all built-in distribution and OS logos.
pub static LOGOS: &[&DistroLogo] = &[
    &LINUX_LOGO,
    &UBUNTU_LOGO,
    &DEBIAN_LOGO,
    &ARCH_LOGO,
    &FEDORA_LOGO,
    &NIXOS_LOGO,
    &ALPINE_LOGO,
    &MANJARO_LOGO,
    &ENDEAVOUROS_LOGO,
    &VOID_LOGO,
    &GENTOO_LOGO,
    &OPENSUSE_LOGO,
    &POP_LOGO,
    &MINT_LOGO,
    &KALI_LOGO,
    &ROCKY_LOGO,
    &ALMA_LOGO,
    &CACHYOS_LOGO,
    &MACOS_LOGO,
    &WINDOWS_LOGO,
    &UNKNOWN_LOGO,
];

/// Returns all supported canonical logo names for `--list-logos`.
pub fn supported_logo_names() -> Vec<&'static str> {
    LOGOS.iter().map(|logo| logo.name).collect()
}

fn find_logo(query: &str) -> Option<&'static DistroLogo> {
    let needle = query.trim();
    if needle.is_empty() {
        return None;
    }

    LOGOS.iter().copied().find(|logo| {
        logo.name.eq_ignore_ascii_case(needle)
            || logo
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(needle))
    })
}

/// Selects the appropriate logo given detected distro metadata and CLI options.
///
/// Resolution order:
/// 1. Returns `None` if `no_logo` is `true` or `logo_type == LogoType::None`.
/// 2. If `override_name` is provided: returns `None` for `"none"`, or looks up
///    the matching logo by name/alias, falling back to `linux` if unrecognized.
/// 3. Looks up `distro_id` (case-insensitive, including aliases).
/// 4. Falls back to whitespace-separated tokens in `id_like`.
/// 5. Falls back to the default `linux` logo.
pub fn select_logo(
    distro_id: &str,
    id_like: Option<&str>,
    override_name: Option<&str>,
    logo_type: LogoType,
    no_logo: bool,
) -> Option<&'static DistroLogo> {
    if no_logo || logo_type == LogoType::None {
        return None;
    }

    if let Some(name) = override_name {
        let trimmed = name.trim();
        if trimmed.eq_ignore_ascii_case("none") {
            return None;
        }
        return find_logo(trimmed).or(Some(&LINUX_LOGO));
    }

    if let Some(logo) = find_logo(distro_id) {
        return Some(logo);
    }

    if let Some(like_list) = id_like {
        for token in like_list.split_whitespace() {
            if let Some(logo) = find_logo(token) {
                return Some(logo);
            }
        }
    }

    Some(&LINUX_LOGO)
}

/// Expands `$N` color tokens in a single logo line and returns the rendered string.
fn expand_line(line: &str, colors: &[&str], no_color: bool) -> String {
    let mut out = String::with_capacity(line.len() + 16);
    let mut chars = line.chars().peekable();
    let mut emitted_color = false;

    while let Some(ch) = chars.next() {
        if ch == '$'
            && let Some(&next) = chars.peek()
            && ('1'..='9').contains(&next)
        {
            chars.next();
            if !no_color {
                let idx = (next as usize) - ('1' as usize);
                if let Some(&ansi) = colors.get(idx) {
                    out.push_str(ansi);
                    emitted_color = true;
                }
            }
            continue;
        }
        out.push(ch);
    }

    if !no_color && emitted_color {
        out.push_str("\x1b[0m");
    }

    out
}

/// Computes visible terminal column width, ignoring ANSI CSI escape sequences.
pub fn visible_width(s: &str) -> usize {
    let mut width = 0;
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for seq_ch in chars.by_ref() {
                if ('@'..='~').contains(&seq_ch) {
                    break;
                }
            }
            continue;
        }
        width += 1;
    }

    width
}

/// Renders a logo's lines with ANSI color expansion (or stripping) and returns
/// `(rendered_lines, max_visible_width)`.
pub fn render_logo_lines(
    logo: &DistroLogo,
    logo_type: LogoType,
    no_color: bool,
) -> (Vec<String>, usize) {
    if logo_type == LogoType::None {
        return (Vec::new(), 0);
    }

    let source_lines = match logo_type {
        LogoType::Small if !logo.small_lines.is_empty() => logo.small_lines,
        _ => logo.lines,
    };

    let mut rendered = Vec::with_capacity(source_lines.len());
    let mut max_width = 0usize;

    for &line in source_lines {
        let expanded = expand_line(line, logo.colors, no_color);
        let width = visible_width(&expanded);
        if width > max_width {
            max_width = width;
        }
        rendered.push(expanded);
    }

    (rendered, max_width)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_names_include_all_required_distros() {
        let names = supported_logo_names();
        for required in [
            "linux",
            "ubuntu",
            "debian",
            "arch",
            "fedora",
            "nixos",
            "alpine",
            "manjaro",
            "endeavouros",
            "void",
            "gentoo",
            "opensuse",
            "pop",
            "mint",
            "kali",
            "rocky",
            "alma",
            "cachyos",
            "macos",
            "windows",
            "unknown",
        ] {
            assert!(
                names.contains(&required),
                "missing required logo '{required}'"
            );
        }
    }

    #[test]
    fn select_logo_handles_case_insensitive_lookup_and_aliases() {
        assert_eq!(
            select_logo("ARCH", None, None, LogoType::Auto, false).map(|l| l.name),
            Some("arch")
        );
        assert_eq!(
            select_logo("archlinux", None, None, LogoType::Auto, false).map(|l| l.name),
            Some("arch")
        );
        assert_eq!(
            select_logo("opensuse-tumbleweed", None, None, LogoType::Auto, false).map(|l| l.name),
            Some("opensuse")
        );
        assert_eq!(
            select_logo("linuxmint", None, None, LogoType::Auto, false).map(|l| l.name),
            Some("mint")
        );
        assert_eq!(
            select_logo("pop_os", None, None, LogoType::Auto, false).map(|l| l.name),
            Some("pop")
        );
    }

    #[test]
    fn select_logo_uses_id_like_and_linux_default_fallback() {
        assert_eq!(
            select_logo("neon", Some("ubuntu debian"), None, LogoType::Auto, false).map(|l| l.name),
            Some("ubuntu")
        );
        assert_eq!(
            select_logo(
                "custom-distro",
                Some("unknown-parent"),
                None,
                LogoType::Auto,
                false
            )
            .map(|l| l.name),
            Some("linux")
        );
    }

    #[test]
    fn select_logo_respects_no_logo_and_overrides() {
        assert_eq!(select_logo("arch", None, None, LogoType::Auto, true), None);
        assert_eq!(select_logo("arch", None, None, LogoType::None, false), None);
        assert_eq!(
            select_logo("arch", None, Some("none"), LogoType::Auto, false),
            None
        );
        assert_eq!(
            select_logo("ubuntu", None, Some("Fedora"), LogoType::Auto, false).map(|l| l.name),
            Some("fedora")
        );
    }

    #[test]
    fn render_logo_lines_expands_tokens_and_computes_exact_width() {
        let logo = DistroLogo {
            name: "sample",
            aliases: &[],
            lines: &["  $1/\\", " $2/  \\"],
            small_lines: &["$1/\\"],
            colors: &["\x1b[1;36m", "\x1b[36m"],
            accent_color: "\x1b[1;36m",
        };

        let (lines, width) = render_logo_lines(&logo, LogoType::Auto, false);
        assert_eq!(lines[0], "  \x1b[1;36m/\\\x1b[0m");
        assert_eq!(lines[1], " \x1b[36m/  \\\x1b[0m");
        assert_eq!(width, 5);

        let (small_lines, small_width) = render_logo_lines(&logo, LogoType::Small, false);
        assert_eq!(small_lines, vec!["\x1b[1;36m/\\\x1b[0m"]);
        assert_eq!(small_width, 2);
    }

    #[test]
    fn render_logo_lines_strips_tokens_when_no_color() {
        let logo = DistroLogo {
            name: "sample",
            aliases: &[],
            lines: &["$1ab$2cd$3ef"],
            small_lines: &[],
            colors: &["\x1b[31m", "\x1b[32m", "\x1b[33m"],
            accent_color: "\x1b[31m",
        };

        let (lines, width) = render_logo_lines(&logo, LogoType::Auto, true);
        assert_eq!(lines, vec!["abcdef"]);
        assert_eq!(width, 6);
    }
}
